# CRIPCODE INDEPENDENCE AUDIT

**Datum:** 2026-09-06 (read-only audit, ništa nije mijenjano)
**Scope:** nezavisnost CripCode-a od originalnog Ship Studio projekta, Vercela, telemetrije, external infrastrukture i vendor zavisnosti.
**Metoda:** grep/code citati na tačnim lokacijama + runtime dokazi iz ove sesije. Sve navodeno je provjereno; neizvesnosti su označene kao NEPOTVRĐENO.

---

## 1. SHIP STUDIO TRAGOVI

### 1.1 Tauri / bundle identitet — GREEN
- `src-tauri/tauri.conf.json`: `productName: "cripcode"`, `identifier: com.fileboin.cripcode`, prozor naslovljen **"Cripcode"**.
- Log direktorij: `C:\Users\bojan\AppData\Local\CripCode\logs` (logging.rs, runtime-proven).
- Boot ekran: "Cripcode didn't start correctly." (Cripcode branding).

### 1.2 Cargo / build identitet — YELLOW (Y2)
- `src-tauri/Cargo.toml`:
  - `name = "ship-studio"` (crate + binary `ship-studio.exe`)
  - lib `ship_studio_lib` (koriste ga svi `ship_studio_lib::` importi)
- Zašto je problem: interni build identitet i dalje nosi originalni naziv; bundle/prozor su rebranded, ali exe/dirs kažu "ship-studio".
- Rješenje: rename paketa + `ship_studio_lib` reference — mehanički posao, radi se u jednom pasusu (nije potrebno za funkcionalnost).

### 1.3 localStorage ključevi — YELLOW (Y3)
- `src/lib/frecency.ts:12` → `ship-studio-palette-frecency`
- `src/components/branches/BranchGraph.tsx:67` → `shipstudio.branchGraphExpanded`
- `src/components/ProjectList.tsx:93` → `ship-studio-project-view-mode`
- Zašto: stari branding u korisničkom storage-u. Bezopasno funkcionalno.
- Rješenje: rename + (opciono) migracija starih ključeva; korisnici gube samo frecency/view-mode postavke.

### 1.4 `.shipstudio` metadata dir u importovanim projektima — YELLOW (Y1, **najvidljiviji trag**)
- `src/components/ImportProject.tsx:26,28,162,203,205,217,239,256,338` → `ensureShipStudioDir`, `ensureGitignoreHasShipstudio`, `shipstudioDir`
- Rust strana: metadata modul kreira `.shipstudio` dir (test: `save_project_metadata_tests::roundtrips_metadata_and_creates_shipstudio_dir`)
- `.gitignore` repozitorija: `.shipstudio/` unos
- Zašto: importovani korisnički projekti dobijaju **Ship Studio-imenovan metadata dir + gitignore unos** — jedini trag koji korisnik vidi u SVOJIM projektima.
- Rješenje: rename → `.cripcode` (kod + testovi + gitignore).

### 1.5 Test fixture-ovi — C (false positive / bezopasno)
- `VisualEditorPanel.test.tsx:44,299,334,359` → `projectPath="/Users/test/ShipStudio/demo"` (test podaci)
- `src-tauri/src/commands/ssh/host_key.rs:633,638` → `91.99.126.64` test fixture (round-trip test)
- `agent_bridge.rs:849` → `"/Users/me/ShipStudio/my site"` (test komentar u produkciji, bez utjecaja)
- Rješenje: nije potrebno (opciono estetsko čišćenje).

### 1.6 Env nazivi — YELLOW (Y6)
- `agent_bridge.rs:180` → `SHIPSTUDIO_DISABLE_AGENT_BRIDGE`
- `error_reporting.rs:46` → `SHIPSTUDIO_BUG_REPORT_FORCE`
- Rješenje: rename uz Y2 (ili zadržati radi kompatibilnosti — interni).

### 1.7 Runtime dokaz (miješano)
- Instalirana stara "Ship Studio" aplikacija na mašini (PID 11048, identifier `com.memberstack.shipstudio`) je **odvojena aplikacija** — ne utječe na CripCode runtime (različiti WebView2 user-data folderi, dokazano tokom sesije).

---

## 2. VERCEL

- **Nema Vercel dependencyja u package.json** (grep: prazno).
- Vercel u CripCodeu je **user-facing feature**: `VercelTokenModal.tsx`, vercel plugin (`plugin_lifecycle.rs:17,1228` → `github.com/fileboin/cripcode-plugin-vercel` — sopstveni plugin repo).
- Kategorija: **opcionalna integracija** (korisnik povezuje vlastiti Vercel token za deploy SVOJIH projekata). Nije infrastrukturna zavisnost CripCode-a.
- **GREEN** (opciona funkcija; nula runtime zavisnosti).

---

## 3. SENTRY / TELEMETRY / ANALYTICS

### 3.1 Analytics (PostHog) — GREEN
- `src-tauri/src/commands/analytics.rs:18-19`: `POSTHOG_API_KEY = ""`, `POSTHOG_HOST = ""` → *"every PostHog send a no-op — no data ever [leaves]"* (fork privacy default, dokumentovano u kodu).
- `src/lib/analytics.ts:166`: `return false; // Default to disabled (privacy-first)`.
- **Runtime-proven ovom sesijom:** `Analytics initialized (enabled: false)` u boot logu.

### 3.2 Error reporting (backend, "admin agent") — GREEN
- `src-tauri/src/error_reporting.rs:35`: `const ENDPOINT: &str = "";` → *"no reports are ever sent"* (fork privacy default, dokumentovano na vrhu fajla).
- Force varijabla: `SHIPSTUDIO_BUG_REPORT_FORCE=1` (Y6 naziv).

### 3.3 Sentry — GREEN (runtime) / YELLOW (Y4, dependency)
- `src/instrument.ts:5`: `const DSN = '';` + guard `if (DSN && (PROD || forceEnabled))` → **Sentry.init se nikad ne izvršava**.
- Scrub helperi (home-dir redakcija) prisutni — dobra praksa.
- `@sentry/react ^10.48.0` i dalje u package.json — **mrtav dependency** (Y4): ukloniti dep + `instrument.ts` (opciono).

### 3.4 Usage tracking
- Analytics ista stvar (3.1) — OFF po defaultu, PostHog ključevi prazni → ništa ne odlazi.
- `track_event` pozivi postoje u kodu, ali svi završe no-op dok je analytics disabled.

**Ukupno privacy: TELEMETRY OFF · ERROR REPORTING OFF (endpoint prazan) · CRASH REPORTING OFF (panic hook → prazan endpoint) · DIAGNOSTICS OFF · REMOTE LOGGING OFF · USAGE TRACKING OFF — SVI provjereni u kodu + runtime.**

---

## 4. EXTERNAL INFRASTRUKTURA

| Stavka | Endpoint/vendor | Status | Kategorija |
|---|---|---|---|
| Updater | `github.com/fileboin/cripcode/releases` + pubkey | sopstveni repo | **potrebno** (feature), GREEN |
| Template API | `api.cripcode.dev` (validacija), S3 AWS (user-config), cdn primjeri u testovima | CripCode infra | **potrebno** (Template API feature), GREEN |
| MCP | lokalni server, port 52203 (runtime-proven) | lokalno | **potrebno** (agent bridge), GREEN |
| cStar support | `@cstar.help/js` + `CSTAR_IDENTITY_SECRET` (option_env) + ChatClient/Library SDK | **opciono** — support ticket feature; vendor: cstar.help | **YELLOW (Y5)** |
| Vercel | user token feature + `cripcode-plugin-vercel` (sopstveni repo) | **opciono** | GREEN |
| Original Ship Studio infra | **nije pronađena** (nema endpointa, servisa, auth/licensing servisa u produkciji) | — | GREEN |
| NEPOTVRĐENO | da li neki opcionalni plugin/API koristi dodatne endpointe van ovih (nije auditirano duboko — van scope-a) | | |

---

## 5. DEPENDENCIES

- `package.json`: `@sentry/react` (mrtav runtime — Y4), `@cstar.help/js` (Y5). **Nema Vercel/Ship-Studio SDK zavisnosti.**
- Ollama/OpenCode/SSH infra: runtime-only, nema package zavisnosti.
- Rust crates: `keyring` (windows-native), pty/tauri ekosistem — nema vendor zavisnosti vezanih za originalni proizvod.
- **NE uklanjati ništa sada** — Y4/Y5 su opcionalni cleanup.

---

## 6. BUILD / PACKAGING

- `tauri.conf.json`: productName/identifier cripcode ✓, `createUpdaterArtifacts: true`, updater endpointi fileboin/cripcode ✓, `externalBin` (ssh-askpass) + `beforeBundleCommand`/`beforeDevCommand` podešeni u ovoj sesiji ✓.
- Windows paths: instalacija/rad dokazani (target\debug + keystore + PrintWindow prozor).
- Build scripts: `copy:askpass`, `dev:askpass` dodani ✓.
- **YELLOW (Y2):** interni exe/crate naziv `ship-studio.exe` / lib `ship_studio_lib` (vidi 1.2).
- NEPOTVRĐENO: NSIS/MSI installer metadata van tauri.conf (nije auditirano dublje — build nije korišten u ovoj sesiji).

---

## 7. UPDATES

- Updater endpointi: **`github.com/fileboin/cripcode/releases/latest/download/latest{,-{{target}}}.json`** + FCC-era pubkey — CripCode vlastiti update kanal.
- Nema originalnog Ship Studio update servera, nema Vercel update zavisnosti, nema trećeg servera.
- **GREEN.**

---

## 8. GITHUB / REPOSITORY IDENTITY

- `package.json` repository → `github.com/fileboin/cripcode` ✓
- Plugin repo → `fileboin/cripcode-plugin-vercel` ✓
- Rust komentari → `github.com/fileboin/cripcode/actions/...` (pty_session.rs:987,990) ✓
- **Sve reference pokazuju na CripCode (fileboin) repozitorij — nula original-owner reference u build/runtime kodu. GREEN.**

---

## 9. LICENCE

- `LICENSE` prisutan u rootu (MIT — package.json `license: MIT`).
- FCC-era repo (referencirani) je također MIT — bez kopi-raight konflikta.
- Nema NOTICE fajla — **opciono** dodati third-party notices kod distribucije.
- **Obaveze:** standardne MIT (zadržati copyright/licencu tekst u distribuciji). Ništa koje blokira fork/rebrand.

---

## 10. FUNCTIONAL INDEPENDENCE — procjena

| Oblast | Boja | Napomena |
|---|---|---|
| Tauri identitet/bundle | GREEN | cripcode potpun |
| Updater | GREEN | vlastiti repo |
| Telemetry/analytics | GREEN | keys prazni + OFF default + runtime |
| Error reporting | GREEN | endpoint prazan |
| Sentry runtime | GREEN | DSN prazan, init ne fire |
| MCP | GREEN | lokalni |
| Template API | GREEN | cripcode.dev + user S3 |
| GitHub identitet | GREEN | fileboin/cripcode |
| SSH/Remote Agent/keystore | GREEN | vlastiti lanac, runtime-proven |
| Cargo build naziv | YELLOW | Y2 |
| `.shipstudio` dir u projektima | YELLOW | Y1 |
| localStorage ključevi | YELLOW | Y3 |
| `@sentry/react` dep | YELLOW | Y4 (mrtav) |
| `@cstar.help/js` support | YELLOW | Y5 (vendor, opcionalna funkcija) |
| `SHIPSTUDIO_*` env nazivi | YELLOW | Y6 |
| Runtime zavisnost od originalne infra | RED | **nema — RED lista prazna** |

---

## CRIPCODE INDEPENDENCE STATUS

**GREEN (9 oblasti):** Tauri identitet/bundle · Updater (vlastiti repo) · Telemetry (OFF, keys prazni) · Error reporting (endpoint prazan) · Sentry runtime (DSN prazan) · MCP (lokalni) · Template API (cripcode.dev/S3) · GitHub identitet (fileboin/cripcode) · SSH/Remote Agent/keystore lanac (vlastiti, proven).

**YELLOW (6):**
- Y1 `.shipstudio` metadata dir u importovanim projektima (`ImportProject.tsx`, metadata modul, `.gitignore`)
- Y2 Cargo/lib naziv `ship-studio`/`ship_studio_lib` (`Cargo.toml`)
- Y3 localStorage ključevi (`frecency.ts:12`, `BranchGraph.tsx:67`, `ProjectList.tsx:93`)
- Y4 `@sentry/react` mrtva zavisnost (`package.json`, `instrument.ts`)
- Y5 `@cstar.help/js` support vendor (`package.json`, `support.ts`, `support.rs`)
- Y6 `SHIPSTUDIO_*` env nazivi (`agent_bridge.rs:180`, `error_reporting.rs:46`)

**RED (0):** nema dokazane zavisnosti koja sprečava nezavisnost.

## PRIORITET 1
- Y1 — jedini korisniku-vidljiv trag (`.shipstudio` dir + gitignore unos u SVAKOM importovanom projektu). Rename → `.cripcode` (ImportProject.tsx + metadata modul + testovi + `.gitignore`).

## PRIORITET 2
- Y2 (Cargo rename, mehanički) · Y3 (localStorage rename + opciona migracija) · Y4 (skiniti `@sentry/react` + `instrument.ts`) · Y6 (env nazivi).

## OPCIONALNO
- Y5 cStar vendor (odluka proizvoda) · stari localStorage ključevi bez migracije · PTY/EOL autogenerated dev-nuspojave · test fixture nazivi.

## ŠTA NE DIRATI (bez potrebe)
- SSH/password/keystore/askpass · host-key security · Remote Agent/GAP 1+2 · Remote Workspace · Ollama/OpenCode konfiguracija · Template API · PTY/EOL autogenerated fajlovi · stash `ee3b3620` · trenutni UI.

## NEPOTVRĐENO → SVE POTVRĐENO (zatvoreno 2026-09-06)
- **Installer metadata — POTVRĐENO ČISTO:** `tauri.conf.json` bundle nema NSIS/MSI/wix/publisher ključeva → Tauri defaulti koriste `productName: cripcode` + `identifier: com.fileboin.cripcode` + deep-link scheme `cripcode`; `tauri.windows.conf.json` sadrži SAMO updater endpoint (fileboin/cripcode). Nula Ship Studio identiteta u installer konfiguraciji. Build nije potreban — metadata je potpuno određen konfigom. (Preostalo poznato Y2: interni exe naziv `ship-studio.exe` = Cargo crate naziv.)
- **cStar SDK — POTVRĐENO INERTNO:** `@cstar.help/js` v0.3.4 (MIT); `src/lib/support.ts` `SUPPORT_CONFIG: SupportConfig | null = null` (komentar: "No CripCode support tenant exists yet... instead of routing support traffic to the former Ship Studio tenant") → LibraryClient/ChatClient konstruktori bacaju "Support integration is not configured" PRIJE bilo kojeg network poziva → **nula saobraćaja prema cstar.help endpointima** (`https://www.cstar.help`). Da je bio aktivan, slao bi: external_id (GitHub login), email, name, timestamp + HMAC-SHA256 potpis (secret u Rust backendu, compile-time env `CSTAR_IDENTITY_SECRET`, po defaultu prazan). Rust `get_support_identity` je praktično dead-code (dostupan samo kroz getChatClient koji baca prvi). Nije uklonjeno, funkcionalnost netaknuta.
- **Third-party licences — POTVRĐENO PERMISSIVE:** Svi 33 frontend runtime dependencyja: MIT ili Apache-2.0/MIT ili MPL-2.0/Apache-2.0 (dompurify, file-level weak copyleft — korišten nemodifikovan, obaveza samo notice). **Nula GPL/AGPL/LGPL** u runtime frontend deps. Rust ekosistem (tauri/tokio/serde/hmac/sha2/keyring/windows) standardno MIT OR Apache-2.0. Obaveza pri distribuciji: zadržati copyright/licence notice (preporučeno: THIRD-PARTY-NOTICES fajl) + MIT copyright notice iz LICENSE.

**KONAČNI STATUS NAKON ZATVARANJA: RED: 0 · UNCONFIRMED: 0 · YELLOW: 6 (nisu blokada) — NEZAVISNOST NIJE UGROŽENA NIČIM.**

---

**COMMIT: NOT DONE · PUSH: NOT DONE · DEPLOY: NOT DONE**

---

## CLEANUP PLAN (ZABILJEŽENO — NIJE IZVRŠENO)

**Odluka:** YELLOW stavke nisu blokada nezavisnosti i NE popravljaju se sada.
**Redoslijed kada se krene u cleanup:**
1. Y1 — `.shipstudio` → `.cripcode` (ImportProject + metadata modul + `.gitignore` + testovi)
2. Y2 — Cargo `ship-studio` → `cripcode` (Cargo.toml + `ship_studio_lib` reference + exe naziv)
3. Y3 — ostali stari Ship Studio nazivi (localStorage ključevi: frecency/branchGraph/view-mode + opciona migracija)
4. Y6 — `SHIPSTUDIO_*` env nazivi (agent_bridge.rs, error_reporting.rs)
5. Y4 — uklanjanje mrtvog `@sentry/react` — SAMO ako se potvrdi da se više ne koristi
6. Y5 — `cstar.help` support vendor — SAMO ako se odluči zamijeniti

**Pravila za cleanup (važe i tada):** NE dirati SSH/Remote Agent · Ollama/OpenCode · Template API · credentials/VPS/deployment · host-key security. Bez širokih audita i refaktora.

**Zabilježeno:** 2026-09-06. Status: NIJE POČETO.