#!/bin/sh
# Print the manifest-list digest of a Docker Hub library image tag, which is
# the value a name@sha256:... reference resolves to on any architecture.
# .reaper.toml must carry a digest (reaper refuses tags); this is how the
# digest is refreshed when the tag in bitbucket-pipelines.yml is bumped.
#
# Usage: sh ci/image-digest.sh [repository[:tag]]     default rust:1.97-trixie
#
# Needs only curl and sed. No pipes: each response lands in a file first, so
# a failed request cannot be mistaken for an empty digest.
set -u

ref=${1:-rust:1.97-trixie}
case $ref in
    *:*) repo=${ref%%:*}; tag=${ref#*:} ;;
    *)   repo=$ref; tag=latest ;;
esac
case $repo in
    */*) ;;
    *) repo=library/$repo ;;
esac

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-digest.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

if ! curl -fsS "https://auth.docker.io/token?service=registry.docker.io&scope=repository:${repo}:pull" > "$tmp/token.json"; then
    echo "image-digest: could not obtain a registry token for $repo" >&2
    exit 1
fi
token=$(sed -n 's/.*"token":"\([^"]*\)".*/\1/p' "$tmp/token.json")
if [ -z "$token" ]; then
    echo "image-digest: token response had no token field" >&2
    exit 1
fi

if ! curl -fsSI -H "Authorization: Bearer $token" \
        -H 'Accept: application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json' \
        "https://registry-1.docker.io/v2/${repo}/manifests/${tag}" > "$tmp/head.txt"; then
    echo "image-digest: manifest request for ${repo}:${tag} failed" >&2
    exit 1
fi
digest=$(sed -n 's/^[Dd]ocker-[Cc]ontent-[Dd]igest: *\(sha256:[0-9a-f]*\).*/\1/p' "$tmp/head.txt")
if [ -z "$digest" ]; then
    echo "image-digest: no Docker-Content-Digest header in the response" >&2
    exit 1
fi
printf 'docker.io/%s@%s\n' "$repo" "$digest"
