#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
BOOK_SRC="$ROOT_DIR/book/src"

mkdir -p "$BOOK_SRC/media"
cp "$ROOT_DIR/docs/media/"* "$BOOK_SRC/media/"

python3 - "$ROOT_DIR" "$BOOK_SRC" "README.md" "blindgate" <<'PY'
import pathlib, re, sys

root = pathlib.Path(sys.argv[1])
book_src = pathlib.Path(sys.argv[2])
intro_rel = sys.argv[3]
repo_name = sys.argv[4]

raw_media_prefix = f"https://raw.githubusercontent.com/david-engelmann/{repo_name}/main/docs/media/"
blob_docs_prefix = f"https://github.com/david-engelmann/{repo_name}/blob/main/docs/"
blob_root_prefix = f"https://github.com/david-engelmann/{repo_name}/blob/main/"

def rewrite_media(text: str) -> str:
    text = text.replace(raw_media_prefix, "media/")
    text = re.sub(r"(?<![A-Za-z0-9_/-])docs/media/", "media/", text)
    return text

intro = (root / intro_rel).read_text(encoding="utf-8")
if "\n## Status\n" in intro:
    intro = intro.split("\n## Status\n", 1)[0].rstrip() + "\n"
intro = rewrite_media(intro)
intro = intro.replace(blob_docs_prefix, "")
for rdoc in ("SECURITY.md", "CONTRIBUTING.md", "CHANGELOG.md"):
    intro = intro.replace(blob_root_prefix + rdoc, rdoc)
intro = intro.replace("(docs/", "(")
(book_src / "introduction.md").write_text(intro, encoding="utf-8")

for doc_name in ("GUIDE.md", "ARCHITECTURE.md", "FORMATS.md", "COMPARISON.md", "FAQ.md"):
    txt = (root / "docs" / doc_name).read_text(encoding="utf-8")
    txt = rewrite_media(txt)
    txt = txt.replace(blob_docs_prefix, "")
    txt = txt.replace("../SECURITY.md", "SECURITY.md")
    txt = txt.replace("../CONTRIBUTING.md", "CONTRIBUTING.md")
    txt = txt.replace("../CHANGELOG.md", "CHANGELOG.md")
    (book_src / doc_name).write_text(txt, encoding="utf-8")

for root_doc in ("CHANGELOG.md", "SECURITY.md", "CONTRIBUTING.md"):
    txt = (root / root_doc).read_text(encoding="utf-8")
    (book_src / root_doc).write_text(txt, encoding="utf-8")
PY
