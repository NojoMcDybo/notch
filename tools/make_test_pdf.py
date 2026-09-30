# Erzeugt eine kleine Test-PDF (5 Seiten) ohne Zusatzpakete — nur fuer Tests, nie echte Dateien anfassen.
import sys

out = sys.argv[1] if len(sys.argv) > 1 else "notch-test.pdf"
pages = 5
objs = []
objs.append(b"<< /Type /Catalog /Pages 2 0 R >>")
kids = " ".join(f"{3 + i * 2} 0 R" for i in range(pages))
objs.append(f"<< /Type /Pages /Kids [{kids}] /Count {pages} >>".encode())
font_id = 3 + pages * 2
for i in range(pages):
    content_id = 4 + i * 2
    objs.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {content_id} 0 R /Resources << /Font << /F1 {font_id} 0 R >> >> >>".encode())
    text = f"BT /F1 36 Tf 80 700 Td (Notch Testdokument) Tj 0 -60 Td /F1 24 Tf (Seite {i + 1} von {pages}) Tj ET"
    objs.append(f"<< /Length {len(text)} >>\nstream\n{text}\nendstream".encode())
objs.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")

data = b"%PDF-1.4\n"
offsets = []
for n, o in enumerate(objs, start=1):
    offsets.append(len(data))
    data += f"{n} 0 obj\n".encode() + o + b"\nendobj\n"
xref = len(data)
data += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
for off in offsets:
    data += f"{off:010d} 00000 n \n".encode()
data += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
open(out, "wb").write(data)
print(out)
