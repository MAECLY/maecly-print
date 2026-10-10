# Encrypted PDF fixtures (FR-059)

Real encrypted PDFs written by macOS's own PDF writer (Quartz `CGPDFContext`),
so the tests do not only check `lopdf` against itself. All three are AES-128
(Standard security handler, V4/R4), two pages, A4.

| File          | User (open) password                 | Owner password | Printing allowed |
| ------------- | ------------------------------------ | -------------- | ---------------- |
| `user.pdf`    | `s3cret`                             | `own3r`        | yes              |
| `owner.pdf`   | none (opens with the empty password) | `own3r`        | yes              |
| `noprint.pdf` | none (opens with the empty password) | `own3r`        | **no**           |

They are test data only: the passwords are public on purpose.

To regenerate them (macOS):

```sh
swiftc -O mkenc.swift -o /tmp/mkenc
/tmp/mkenc user.pdf    s3cret own3r 1
/tmp/mkenc owner.pdf   ""     own3r 1
/tmp/mkenc noprint.pdf ""     own3r 0
```

RC4 and AES-256 cases, and a file whose objects live in object streams, are
generated inside the tests with `lopdf`.
