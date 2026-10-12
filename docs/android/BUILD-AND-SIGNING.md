# CripCode Android — Build & Signing

Everything needed to produce an APK, and the rules for keeping signing material
out of the repository. Read [README.md](README.md) first.

---

## Toolchain requirements (verified from the committed Gradle configuration)

| Component | Version | Source |
|---|---|---|
| Gradle | **8.14.3** | `src-tauri/gen/android/gradle/wrapper/gradle-wrapper.properties` |
| Android Gradle Plugin | **8.11.0** | `src-tauri/gen/android/build.gradle.kts` |
| Kotlin plugin | **1.9.25** | same |
| JDK | **17** (AGP 8.x requirement) | — |
| Android `compileSdk` / `targetSdk` | **36** | `app/build.gradle.kts` |
| Android `minSdk` | **24** | same |
| Kotlin `jvmTarget` | **1.8** | same |
| NDK | required — the Rust NDK targets are built through Gradle | |

### Required Rust targets

`src-tauri/gen/android/buildSrc/.../RustPlugin.kt` declares the default target
list. For a universal build all four are compiled:

| Rust target | Android ABI |
|---|---|
| `aarch64-linux-android` | `arm64-v8a` |
| `armv7-linux-androideabi` | `armeabi-v7a` |
| `i686-linux-android` | `x86` |
| `x86_64-linux-android` | `x86_64` |

Narrowing with `-p targetList=… -p abiList=…` speeds up a debug iteration but
changes the shipped ABI coverage — do not narrow a release build without saying
so.

### Android-only Rust dependencies

`src-tauri/Cargo.toml`, `[target.'cfg(target_os = "android")'.dependencies]`:

- `russh = 0.63.3` — `default-features = false`, features `ring`, `rsa`,
  `flate2`. **`ring`, not `aws-lc-rs`, on purpose:** it keeps the NDK build free
  of a C toolchain.
- `jni = 0.21` and `ndk-context = 0.1` — the in-app update's package-installer
  and FileProvider handoff.

---

## Build commands

The Android build is driven by the **Tauri CLI**, which generates
`src-tauri/gen/android/` and writes `gen/android/app/tauri.properties` before
Gradle runs. There is **no `pnpm android:*` script** in `package.json`; use the
CLI directly.

```bash
# from the repository root
pnpm tauri android build --apk
pnpm tauri android build --aab
```

These run, in order:

1. `beforeBuildCommand` → `node scripts/conditional-build.mjs`
2. Gradle, which runs the Rust cargo build for every ABI target
3. `beforeBundleCommand` → `pnpm copy:askpass` (desktop only in practice)

> **Honest note on the exact invocation:** the precise flag combination used for
> the existing artifacts is **not recorded anywhere in the repository**. The
> artifacts prove a `universalRelease` APK with `versionCode 18020` /
> `versionName 0.18.21` was produced, and the configuration above is what
> produces one. Treat the commands above as the correct entry point, not as a
> transcript of a past run.

### The conditional frontend build

`src-tauri/tauri.conf.json` sets `beforeBuildCommand` to
`node scripts/conditional-build.mjs` instead of `pnpm build`. Reason: Vite
always rewrites `dist` with fresh mtimes, and Tauri embeds `dist` into the Rust
binary with `include_bytes!`, so an unchanged frontend still forced a full
20–45 minute Rust recompile on every Android build.

The script hashes everything that can change the Vite/tsc output (sources,
configs, lockfile, `public/`, `.env*`, and every `VITE_*` environment variable)
and skips `pnpm build` when the existing `dist` came from exactly that input.

```bash
# check the decision without building
node scripts/conditional-build.mjs --dry-run
```

The manifest lives at `node_modules/.cache/cripcode-build-hash`. If it is ever
stale or suspect, delete it and the next build runs in full.

---

## Output paths

| Variant | Path |
|---|---|
| Debug APK | `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` |
| Release APK | `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk` |
| Release AAB | `src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab` |
| R8 mapping | `src-tauri/gen/android/app/build/outputs/mapping/universalRelease/mapping.txt` |

Existing artifacts (see [TEST-RESULTS.md](TEST-RESULTS.md)):

| Artifact | Size | Built |
|---|---|---|
| `app-universal-release.apk` | ~205 MB | 2026-09-29 |
| `app-universal-debug.apk` | ~445 MB | 2026-09-24 |
| `app-universal-release.aab` | ~24.6 MB | 2026-09-24 |

The debug APK is far larger because debug builds keep native debug symbols
(`jniLibs.keepDebugSymbols` for all four ABIs).

---

## Debug vs signed release

| | Debug | Release |
|---|---|---|
| Signing | Android debug key | **Release keystore**, when `keystore.properties` exists |
| Minification | off | **R8 on**, `proguard-android-optimize.txt` |
| Cleartext traffic | **allowed** (`usesCleartextTraffic=true`) | blocked (`false`) |
| JNI debug | on | off |
| Installable over a release build? | **No** | Yes |

**A debug APK cannot be installed over a signed release install** — Android
refuses the signature mismatch. Debug builds are for iteration only. If a device
already has a release build installed, uninstall it first or ship a release APK.

### Signing configuration

`src-tauri/gen/android/app/build.gradle.kts` (uncommitted) defines a `release`
signing config that is applied **only when `keystore.properties` exists**:

```kotlin
signingConfigs {
    create("release") {
        val keystorePropertiesFile = rootProject.file("keystore.properties")
        if (keystorePropertiesFile.exists()) { /* load keyAlias, password, storeFile */ }
    }
}
buildTypes {
    getByName("release") {
        if (rootProject.file("keystore.properties").exists()) {
            signingConfig = signingConfigs.getByName("release")
        }
    }
}
```

The keystore properties file reads exactly four keys: `keyAlias`, `password`,
`storeFile`.

> **This block is uncommitted.** On the committed tree, a release build is
> unsigned. Do not assume a signed release build exists because the old APK on
> disk was signed.

---

## Secrets

> ### ⚠️ Hard rules
>
> 1. **Never commit** `keystore.properties`, the `.jks`/`.keystore` file, or any
>    password. No exceptions, no "it's a test key".
> 2. **Never print** them — not into a log, a terminal transcript, a commit
>    message, a documentation file, an issue, or a chat message.
> 3. **Never pass them as command-line arguments** or environment variables
>    visible to other processes, and never bake them into a source file.
> 4. **Never put a real host, IP, username or private key** into documentation.
>
> `src-tauri/gen/android/keystore.properties` is listed in
> `src-tauri/gen/android/.gitignore`, alongside `local.properties`,
> `key.properties`, `/build`, `/.cxx` and `/local.properties`.
>
> **If a signing credential is ever exposed in a log, a terminal transcript, or
> a commit — stop and tell the owner immediately.** Rotating a release key
> invalidates every previously published APK's update path.

---

## Versioning

Three files must agree. They currently **do not** — see
[KNOWN-ISSUES.md §5](KNOWN-ISSUES.md#5-version-skew-between-cargotoml-and-tauriconfjson).

| File | Field | Current value |
|---|---|---|
| `src-tauri/tauri.conf.json` | `version` | `0.18.21` |
| `src-tauri/tauri.conf.json` | `bundle.android.versionCode` | `18020` |
| `src-tauri/Cargo.toml` | `package.version` | `0.18.18` ← **mismatch** |
| `src-tauri/gen/android/app/tauri.properties` | `tauri.android.versionCode` / `versionName` | `18020` / `0.18.21` *(generated — do not hand-edit)* |

### How the update comparison works

1. The Tauri CLI writes `tauri.android.versionCode` into
   `gen/android/app/tauri.properties`, then invokes cargo.
2. `src-tauri/build.rs` reads that file on Android targets and re-exports it as
   `cargo:rustc-env=ANDROID_VERSION_CODE=<n>`.
3. `android_update::local_version_code()` reads `ANDROID_VERSION_CODE` at compile
   time, falling back to the Tauri semver formula
   `major * 1_000_000 + minor * 1_000 + patch` when the variable is absent.

The comparison is **strictly greater**. Equal or lower never prompts. This is
why the *installed app's* version must come from the Android build, not from a
display label.

### Bumping a version

1. Set `version` in `src-tauri/tauri.conf.json`.
2. Set `version` in `src-tauri/Cargo.toml` to the same value.
3. Increase `bundle.android.versionCode` in `src-tauri/tauri.conf.json`.
4. Rebuild, and publish the matching `android-latest.json` metadata
   (`version_name`, `version_code`, `apk_url`, `sha256`) alongside the APK.

An APK that keeps the same `versionCode` will **never** be offered as an update.

---

## Verification procedure

After every Android build:

```bash
# 1. The artifact exists and carries the expected identity
node -e "console.log(require('./src-tauri/gen/android/app/build/outputs/apk/universal/release/output-metadata.json'))"
#    expect applicationId com.fileboin.cripcode, and versionCode / versionName
#    matching tauri.conf.json

# 2. Android-only Rust code type-checked (CI never does this)
pnpm tauri android build --apk        # or the Gradle invocation above

# 3. Shared gates still green
pnpm check:all && pnpm test:run && pnpm rust:test
```

Step 2 is the one people forget: **`cargo test` on the desktop host does not
compile `#[cfg(target_os = "android")]` blocks.** A green CI run says nothing
about whether Android still compiles.

For release signing, additionally confirm with the SDK's own tool (path from
`local.properties`), never by inspecting the file yourself:

```bash
apksigner verify --print-certs <path-to.apk>
```

---

## Related documents

- [README.md](README.md)
- [ARCHITECTURE.md](ARCHITECTURE.md)
- [IMPLEMENTATION-HISTORY.md](IMPLEMENTATION-HISTORY.md)
- [TEST-RESULTS.md](TEST-RESULTS.md)
- [KNOWN-ISSUES.md](KNOWN-ISSUES.md)
- [NEXT-STEPS.md](NEXT-STEPS.md)