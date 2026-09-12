//! The server loop: LSP over stdio, one request at a time.
//!
//! `lsp-server` is the transport and nothing more -- framing and JSON-RPC,
//! the crate rust-analyzer speaks through -- so this is a plain loop over
//! messages with no async runtime anywhere. The reasoning behind that
//! choice is in `docs/issues/0006`; the short of it is that rue has no
//! tokio, judging a text takes a millisecond, and an editor that asks
//! twice gets two answers in order.

use std::error::Error;

use lsp_server::{Connection, ExtractError, Message, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification,
    PublishDiagnostics,
};
use lsp_types::request::{HoverRequest, Request as LspRequest};
use lsp_types::{
    HoverProviderCapability, InitializeParams, OneOf, PublishDiagnosticsParams, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};

use crate::{hover, judge, path_of, Documents};

/// What the server tells an editor it can do. Full-text sync, because a
/// rue file is small and an incremental sync is a second source of truth
/// about the buffer; hover; and nothing else, so an editor never asks for
/// something answered with a shrug.
pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(false)),
        ..Default::default()
    }
}

/// Run the server over stdio until the editor says to stop.
pub fn run() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();
    let caps = serde_json::to_value(capabilities())?;
    let params = connection.initialize(caps)?;
    let _params: InitializeParams = serde_json::from_value(params)?;
    main_loop(&connection)?;
    io_threads.join()?;
    Ok(())
}

fn main_loop(connection: &Connection) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut docs = Documents::default();
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                if let Some(response) = handle_request(&docs, req) {
                    connection.sender.send(Message::Response(response))?;
                }
            }
            Message::Notification(note) => {
                let published = handle_notification(&mut docs, note);
                for p in published {
                    connection.sender.send(Message::Notification(
                        lsp_server::Notification::new(PublishDiagnostics::METHOD.to_string(), p),
                    ))?;
                }
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// One request. `None` is a method this server does not serve, which the
/// capabilities already said.
pub fn handle_request(docs: &Documents, req: Request) -> Option<Response> {
    let id = req.id.clone();
    match cast::<HoverRequest>(req) {
        Ok((id, params)) => {
            let url = params.text_document_position_params.text_document.uri;
            let at = params.text_document_position_params.position;
            let result = path_of(&url)
                .zip(docs.text(&url))
                .and_then(|(path, text)| hover(&path, text, at));
            Some(Response::new_ok(id, result))
        }
        Err(ExtractError::MethodMismatch(_)) => Some(Response::new_ok(id, serde_json::Value::Null)),
        Err(ExtractError::JsonError { .. }) => None,
    }
}

/// One notification, and the diagnostics it produces. A document that
/// closed publishes an empty list: what an editor shows must go when the
/// text it was about is gone.
pub fn handle_notification(
    docs: &mut Documents,
    note: lsp_server::Notification,
) -> Vec<PublishDiagnosticsParams> {
    match note.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let Ok(p) = serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(note.params)
            else {
                return Vec::new();
            };
            let url = p.text_document.uri.clone();
            docs.open(url.clone(), p.text_document.text);
            publish(docs, &url)
        }
        DidChangeTextDocument::METHOD => {
            let Ok(p) =
                serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(note.params)
            else {
                return Vec::new();
            };
            let url = p.text_document.uri.clone();
            // Full sync: the last change carries the whole text.
            if let Some(change) = p.content_changes.into_iter().next_back() {
                docs.change(url.clone(), change.text);
            }
            publish(docs, &url)
        }
        DidCloseTextDocument::METHOD => {
            let Ok(p) =
                serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(note.params)
            else {
                return Vec::new();
            };
            docs.close(&p.text_document.uri);
            vec![PublishDiagnosticsParams {
                uri: p.text_document.uri,
                diagnostics: Vec::new(),
                version: None,
            }]
        }
        _ => Vec::new(),
    }
}

fn publish(docs: &Documents, url: &Uri) -> Vec<PublishDiagnosticsParams> {
    let Some((path, text)) = path_of(url).zip(docs.text(url)) else {
        return Vec::new();
    };
    let judgment = judge(&path, text);
    vec![PublishDiagnosticsParams {
        uri: url.clone(),
        diagnostics: judgment.diagnostics,
        version: None,
    }]
}

fn cast<R>(req: Request) -> Result<(RequestId, R::Params), ExtractError<Request>>
where
    R: LspRequest,
    R::Params: serde::de::DeserializeOwned,
{
    req.extract(R::METHOD)
}
