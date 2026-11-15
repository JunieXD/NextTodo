# Release builds are shrunk and optimized by R8 (docs/android.md#构建和签名), but not renamed: the
# app is open source, and stack traces in logs stay readable without a mapping file per release.
-dontobfuscate
