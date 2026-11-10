#!/bin/bash
# Makes the real documents the tests in pdf_real.rs read, with LibreOffice and
# qpdf: `make.sh DIR [ROWS]` writes DIR/sheet.pdf, book.pdf, images.pdf and
# deck.pdf as LibreOffice exports them, and DIR/*.os.pdf, the same with their
# objects in object streams.
set -e
dir=${1:?usage: make.sh DIR [ROWS]}
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$dir/profile"
python3 -I "$here/make.py" "$dir" "${2:-6000}"
cd "$dir"
soffice --headless "-env:UserInstallation=file://$dir/profile" --convert-to pdf sheet.csv book.txt > /dev/null
soffice --headless "-env:UserInstallation=file://$dir/profile" --infilter="HTML (StarWriter)" --convert-to pdf images.html deck.html > /dev/null
for name in sheet book images deck; do
    qpdf --object-streams=generate --compression-level=9 "$name.pdf" "$name.os.pdf"
done
