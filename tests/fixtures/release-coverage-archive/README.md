# Independent ZIP parser control

`dotnet-deflate.zip` was produced with the already available PowerShell/.NET `System.IO.Compression.ZipArchive` writer, not the production reader or the Node fixture ZIP builder. It contains only two harmless UTF-8 files: `windows-native-coverage.json` with `{"fixture":"independent-dotnet-ziparchive"}`, and `logs/result.log` with `independent fixture output` followed by LF. This is a parser control, not a native coverage report or runtime acceptance evidence.

Both entries use `CompressionLevel.Optimal` and fixed entry time `2026-10-09T00:00:00Z`; their CRC/compression/local and central headers are generated independently by the standard library. Normal CI tests read this committed fixture with Node and do not require .NET or PowerShell to regenerate it.

The Node-generated effect-free fixtures separately cover stored entries and streaming data descriptors, matching the ordinary streaming ZIP format used by the [GitHub Actions artifact uploader](https://github.com/actions/toolkit/blob/main/packages/artifact/src/internal/upload/zip.ts). Unsupported ZIP64, multidisk and encrypted archives fail closed. No application or native test executable was launched to make this control.
