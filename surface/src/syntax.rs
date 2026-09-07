//! The syntax kinds of the lossless tree (docs/ROADMAP.md 6.2 and 6.3):
//! every token the lexer produces and every node the parser builds, one
//! flat enumeration as rowan wants it. Keywords are contextual: the lexer
//! yields a `NAME` and the parser reads its text, because most keywords
//! (`user`, `content`, `set`, `window`) are also ordinary keyword-argument
//! names.

use rowan::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // --- tokens: trivia
    WHITESPACE,
    NEWLINE,
    COMMENT,
    // --- tokens: words and literals
    NAME,
    UPPER_NAME,
    ATOM,
    INT,
    FLOAT,
    DURATION,
    STRING,
    // --- tokens: punctuation
    L_PAREN,
    R_PAREN,
    L_BRACK,
    R_BRACK,
    L_BRACE,
    R_BRACE,
    PERCENT_BRACE,
    COMMA,
    COLON,
    PIPE,
    PIPE_GT,
    EQ,
    EQ_EQ,
    BANG_EQ,
    LT,
    LE,
    GT,
    GE,
    PLUS,
    MINUS,
    STAR,
    SLASH,
    PERCENT,
    DOT,
    /// A character the lexer does not know.
    ERROR_TOKEN,
    // --- nodes
    FILE,
    VERSION,
    IMPORT,
    /// `site do ... end`, `operators do ... end`, `hooks do ... end`,
    /// `par do ... end`, `preflight do ... end`: a keyword and a body.
    BLOCK,
    /// `defprobe`, `defprim`, `defop`, `defplan`, `defrole`, `defprotocol`,
    /// `defimpl`: a keyword, a header, a body.
    DEF,
    HEADER,
    PATTERN,
    BODY,
    /// A keyword line: the keyword, then arguments.
    LINE,
    STEP,
    PIPELINE,
    KNELL,
    SLOT,
    OBSERVE,
    ASSERT,
    REPEAT,
    WHEN,
    ELSE_ARM,
    /// A `defrole` contribution: a slot atom, an optional priority, an item.
    CONTRIBUTION,
    ARGS,
    KW,
    CALL,
    REF,
    BINARY,
    UNARY,
    PAREN,
    LIST,
    RECORD,
    RECORD_ENTRY,
    LITERAL,
    ERROR,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT
        )
    }

    /// The name a diagnostic uses for a token of this kind.
    pub fn describe(self) -> &'static str {
        use SyntaxKind::*;
        match self {
            WHITESPACE => "whitespace",
            NEWLINE => "end of line",
            COMMENT => "a comment",
            NAME => "a name",
            UPPER_NAME => "an upper-case name",
            ATOM => "an atom",
            INT => "an integer",
            FLOAT => "a number",
            DURATION => "a duration",
            STRING => "a string",
            L_PAREN => "`(`",
            R_PAREN => "`)`",
            L_BRACK => "`[`",
            R_BRACK => "`]`",
            L_BRACE => "`{`",
            R_BRACE => "`}`",
            PERCENT_BRACE => "`%{`",
            COMMA => "`,`",
            COLON => "`:`",
            PIPE => "`|`",
            PIPE_GT => "`|>`",
            EQ => "`=`",
            EQ_EQ => "`==`",
            BANG_EQ => "`!=`",
            LT => "`<`",
            LE => "`<=`",
            GT => "`>`",
            GE => "`>=`",
            PLUS => "`+`",
            MINUS => "`-`",
            STAR => "`*`",
            SLASH => "`/`",
            PERCENT => "`%`",
            DOT => "`.`",
            ERROR_TOKEN => "an unknown character",
            _ => "a node",
        }
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(k: SyntaxKind) -> Self {
        rowan::SyntaxKind(k as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RueLanguage {}

impl Language for RueLanguage {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(
            raw.0 <= SyntaxKind::ERROR as u16,
            "unknown syntax kind {}",
            raw.0
        );
        // SAFETY-free: the enumeration is repr(u16), contiguous from 0, and
        // ERROR is its last member; the assertion above holds the range.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }
    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<RueLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<RueLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<RueLanguage>;
