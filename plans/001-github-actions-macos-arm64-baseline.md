# Plan 001: Reproduzierbaren unsigned macOS-ARM64-Baseline-Build in GitHub Actions einrichten

> **Anweisungen für den ausführenden Agenten**: Diesen Plan Schritt für
> Schritt ausführen. Jedes Prüfkommando ausführen und das erwartete Ergebnis
> bestätigen, bevor es weitergeht. Bei einer Bedingung aus „STOP-Bedingungen“
> anhalten und berichten; nicht improvisieren. Externe Zustandsänderungen
> (Commit, Push, Pull Request, Aktivierung oder Start eines Workflows) nur nach
> der jeweils ausdrücklich genannten Freigabe durchführen. Nach erfolgreichem
> Abschluss die Statuszeile dieses Plans in `plans/README.md` aktualisieren.
>
> **Drift-Prüfung (zuerst ausführen)**:
>
> ```bash
> git diff --stat 57456f0b..HEAD -- \
>   .github/workflows/flutter-build.yml \
>   .github/workflows/bridge.yml \
>   build.py Cargo.toml vcpkg.json
> ```
>
> Erwartung: keine Ausgabe. Bei Ausgabe die unten zitierten Stellen mit dem
> Live-Code vergleichen. Bei einer inhaltlichen Abweichung an den
> macOS-/Bridge-Buildschritten anhalten und den Plan neu prüfen lassen.

## Status

- **Priorität**: P1
- **Aufwand**: M
- **Risiko**: MED
- **Abhängig von**: keine
- **Kategorie**: dx
- **Geplant auf**: Commit `57456f0b`, 25.07.2026

## Warum dieser Schritt wichtig ist

Bevor Branding oder Serverkonfiguration geändert werden, braucht der Fork
einen nachweislich funktionierenden und wiederholbaren Build. Der erste
Workflow soll deshalb ausschließlich RustDesk 1.4.9 für Apple Silicon bauen,
ein unsigned DMG als kurzlebiges GitHub-Artefakt bereitstellen und nichts
veröffentlichen. So lassen sich Toolchain, Fork, Submodule und GitHub Runner
isoliert validieren, ohne auf dem lokalen Mac zu kompilieren und ohne
Signing-Secrets einzuführen.

„Unsigned“ bedeutet hier: keine eigene Developer-ID-Signatur und keine
Apple-Notarisierung. Das Ergebnis ist ein Lern- und Prüfartefakt, noch kein
kundentauglicher Installer.

## Aktueller Zustand

### Verifizierte Tatsachen

- Das öffentliche Fork-Repository ist
  `https://github.com/ongrowww/rustdesk-lab`.
- Lokales `origin` zeigt auf den Fork; `upstream` liest von
  `https://github.com/rustdesk/rustdesk.git`. Pushes zu `upstream` sind lokal
  mit der Push-URL `DISABLED` blockiert.
- `master` stand bei der Planung auf `57456f0b`.
- Der stabile Tag `1.4.9` zeigt auf den vollständigen Commit
  `6c578292e8ebbbec708b76986ba8c4bc7c509747`.
- `Cargo.toml:2-3` enthält:

  ```toml
  name = "rustdesk"
  version = "1.4.9"
  ```

- Der Fork ist öffentlich. Laut GitHub-Dokumentation sind Standard-Runner in
  öffentlichen Repositories kostenlos und unbegrenzt; `macos-14` ist ein
  ARM64/M1-Label. Referenz:
  <https://docs.github.com/en/actions/reference/runners/github-hosted-runners>
- Die Actions-Repository-Berechtigung ist aktiviert, und die
  Standard-Workflowberechtigung ist `read`. Die Actions-API meldete bei der
  Planung jedoch `total_count: 0`, obwohl Workflowdateien im Fork vorhanden
  sind. Deshalb ist eine einmalige Fork-Aktivierung beziehungsweise
  Registrierung nach dem Merge ausdrücklich zu prüfen.

### Relevante Upstream-Buildlogik

- `.github/workflows/flutter-build.yml:20-47` fixiert für 1.4.9 unter anderem:

  ```yaml
  RUST_VERSION: "1.75"
  MAC_RUST_VERSION: "1.81"
  FLUTTER_VERSION: "3.24.5"
  VCPKG_COMMIT_ID: "120deac3062162151622ca4860575a33844ba10b"
  VERSION: "1.4.9"
  ```

- `.github/workflows/flutter-build.yml:650-672` verwendet für Apple Silicon
  den Runner `macos-14`, das Target `aarch64-apple-darwin`, den
  vcpkg-Triplet `arm64-osx` und das Buildargument `--screencapturekit`.
- `.github/workflows/flutter-build.yml:719-817` installiert LLVM,
  `create-dmg`, `pkg-config`, NASM 2.16.03, Flutter 3.24.5, Rust 1.81 und
  vcpkg. Danach werden für ARM64 die macOS-Mindestversionen auf 12.3 gesetzt
  und ausgeführt:

  ```bash
  ./build.py --flutter --hwcodec --unix-file-copy-paste --screencapturekit
  ```

- `.github/workflows/flutter-build.yml:819-832` erzeugt bereits ein unsigned
  DMG und lädt es als Artefakt hoch. Die nachfolgenden Zeilen 834-866 enthalten
  dagegen Signier-, Notarisierungs- und Release-Logik; sie dürfen nicht in den
  Lab-Workflow übernommen werden.
- `.github/workflows/bridge.yml:8-11` fixiert
  `cargo-expand` 1.0.95, `flutter_rust_bridge_codegen` 1.80.1 und Rust 1.75.
  Die Standard-Bridge wird mit Flutter 3.22.3 generiert
  (`bridge.yml:20-27,79-104`).
- Die generierten Bridge-Dateien sind absichtlich ignoriert und im Checkout
  nicht vorhanden:

  ```text
  src/bridge_generated.rs
  src/bridge_generated.io.rs
  flutter/lib/generated_bridge.dart
  flutter/lib/generated_bridge.freezed.dart
  flutter/macos/Runner/bridge_generated.h
  ```

  Der neue Workflow muss sie daher vor dem macOS-Build in einem separaten Job
  erzeugen und als internes Artefakt übertragen.
- `build.py:405-426` baut auf macOS zunächst die Rust-Dylib, beschränkt den
  Flutter/Xcode-Build auf die Hostarchitektur und erzeugt
  `flutter/build/macos/Build/Products/Release/RustDesk.app`.
- Die Upstream-Workflows pinnen Drittanbieter-Actions bereits auf vollständige
  Commit-SHAs. Diesen Stil beibehalten. Git-Committexte folgen überwiegend
  Conventional Commits, beispielsweise
  `feat(macos): silent auto-update with security hardening (#15550)`.
  `docs/CONTRIBUTING.md` verlangt außerdem DCO-Sign-off.

### Betreiberziel und vorgeschlagene Grenze

- Betreiberziel: Der Build läuft vollständig auf GitHub, nicht auf dem
  lokalen MacBook.
- Vorschlag dieses Plans: ausschließlich manueller Trigger, exakt ein
  Apple-Silicon-DMG, keine Secrets, kein Release, keine automatische
  Ausführung und sieben Tage Artefaktaufbewahrung.

## Benötigte Kommandos

Alle Kommandos werden im Repository
`/Users/schrobo/Developer/ongrow/rustdesk-lab` ausgeführt.

| Zweck | Kommando | Erwartung bei Erfolg |
|-------|----------|-----------------------|
| Status | `git status --short --branch` | aktueller Branch und nur bekannte Planänderungen |
| Tag prüfen | `git rev-parse 1.4.9^{commit}` | exakt `6c578292e8ebbbec708b76986ba8c4bc7c509747` |
| YAML parsen | `ruby -e 'require "yaml"; YAML.parse_file(ARGV.fetch(0)); puts "YAML_OK"' .github/workflows/ongrow-lab-macos-arm64.yml` | `YAML_OK`, Exit 0 |
| Whitespace prüfen | `ruby -e 's=File.binread(ARGV.fetch(0)); abort "NO_FINAL_NEWLINE" unless s.end_with?("\n"); s.each_line.with_index(1){|l,n| abort "TRAILING_WHITESPACE:#{n}" if l.match?(/[ \t]+\n\z/)}; puts "WHITESPACE_OK"' .github/workflows/ongrow-lab-macos-arm64.yml` | `WHITESPACE_OK`, Exit 0 |
| Remote-Workflow prüfen | `gh workflow view ongrow-lab-macos-arm64.yml --repo ongrowww/rustdesk-lab` | Workflowname und `workflow_dispatch` sichtbar |
| Remote-Build starten | `gh workflow run ongrow-lab-macos-arm64.yml --repo ongrowww/rustdesk-lab --ref master` | Meldung über gestarteten Workflow, Exit 0 |
| Remote-Build verfolgen | `gh run watch <RUN_ID> --repo ongrowww/rustdesk-lab --exit-status` | Abschluss `success`, Exit 0 |

Für diesen Plan gibt es bewusst keinen lokalen Rust-/Flutter-Build. Der
vollständige Build und die Artefaktprüfung sind die Remote-Verifikation.

## Umfang

**Im Umfang – die einzigen Implementierungsdateien, die geändert werden
dürfen:**

- `.github/workflows/ongrow-lab-macos-arm64.yml` (neu)
- `plans/README.md` (nur Statuspflege)

**Nur als Referenz lesen, nicht ändern:**

- `.github/workflows/flutter-build.yml`
- `.github/workflows/bridge.yml`
- `build.py`
- `Cargo.toml`
- `vcpkg.json`

**Außerhalb des Umfangs – nicht ändern:**

- sämtliche Rust-, Dart-, Flutter-, Plattform- und Ressourcendateien
- bestehende Upstream-Workflows
- Branding, Namen, Icons und Bundle-Identifier
- Server-, API-, Rendezvous-, Relay- oder Update-Konfiguration
- unattended access, Passwörter und Berechtigungslogik
- Apple Developer ID, Code Signing, Notarisierung und Secrets
- Releases, Tags und automatische Updatekanäle
- Windows-, Linux-, Android-, iOS- und Intel-macOS-Builds
- Blacksmith-, VPS- oder Self-hosted-Runner-Konfiguration

## Git-Arbeitsweise

- Arbeitsbranch: `lab/001-macos-arm64-baseline`
- Vom aktuellen, geprüften `master` abzweigen. Der Workflow selbst checkt für
  beide Jobs den unveränderlichen 1.4.9-Commit aus; der Arbeitsbranch muss
  deshalb nicht vom Tag abzweigen.
- Commitstil: Conventional Commit mit DCO-Sign-off, vorgeschlagener Text:
  `ci: add macOS arm64 baseline workflow`
- Den Plan und die Workflowänderung nicht ungefragt committen, pushen oder als
  Pull Request veröffentlichen.
- Push, Pull Request und Merge sind Freigabepunkt 1.
- Aktivierung vorhandener Workflows und Start des Builds sind
  Freigabepunkt 2.

## Schritte

### Schritt 1: Ausgangszustand und unveränderliche Buildquelle bestätigen

1. Drift-Prüfung vom Anfang des Plans ausführen.
2. Status, Remotes und Tag prüfen:

   ```bash
   git status --short --branch
   git remote -v
   git rev-parse 1.4.9^{commit}
   git show 1.4.9:Cargo.toml | sed -n '1,5p'
   ```

3. Wenn der Arbeitsbranch noch nicht existiert und die Arbeitskopie keine
   fremden Änderungen enthält:

   ```bash
   git switch -c lab/001-macos-arm64-baseline
   ```

   Bereits vorhandene `plans/`-Dateien sind zu erhalten.

**Prüfen**:

```bash
test "$(git rev-parse 1.4.9^{commit})" = \
  "6c578292e8ebbbec708b76986ba8c4bc7c509747"
git branch --show-current
```

Erwartung: erstes Kommando Exit 0; zweites Kommando gibt
`lab/001-macos-arm64-baseline` aus.

### Schritt 2: Einen isolierten manuellen Workflow anlegen

`.github/workflows/ongrow-lab-macos-arm64.yml` neu anlegen. Die Datei muss
folgende Top-Level-Grenzen enthalten:

```yaml
name: OnGROW Lab - macOS ARM64 Baseline

on:
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: ongrow-lab-macos-arm64-baseline
  cancel-in-progress: false
```

Es darf keine Trigger für `push`, `pull_request`, `schedule`, Releases oder
Tags geben. Es darf keine Referenz auf `secrets` geben.

Folgende Werte als top-level `env` exakt fixieren:

```yaml
SOURCE_SHA: "6c578292e8ebbbec708b76986ba8c4bc7c509747"
VERSION: "1.4.9"
BRIDGE_RUST_VERSION: "1.75"
MAC_RUST_VERSION: "1.81"
BRIDGE_FLUTTER_VERSION: "3.22.3"
FLUTTER_VERSION: "3.24.5"
CARGO_EXPAND_VERSION: "1.0.95"
FLUTTER_RUST_BRIDGE_VERSION: "1.80.1"
VCPKG_COMMIT_ID: "120deac3062162151622ca4860575a33844ba10b"
VCPKG_BINARY_SOURCES: "clear;x-gha,readwrite"
```

Der Workflow hat genau zwei Jobs.

#### Job `generate-bridge`

- `runs-on: ubuntu-22.04`
- `timeout-minutes: 90`
- Checkout exakt `SOURCE_SHA`, rekursive Submodule,
  `persist-credentials: false`.
- Direkt danach muss ein Shellschritt bestätigen:

  ```bash
  test "$(git rev-parse HEAD)" = "$SOURCE_SHA"
  test "$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)" = "$VERSION"
  ```

- Voraussetzungen wie in `.github/workflows/bridge.yml:42-59` installieren:
  `ca-certificates`, `clang`, `cmake`, `curl`, `gcc`, `git`, `g++`,
  `libclang-dev`, `libgtk-3-dev`, `llvm-dev`, `nasm`, `ninja-build`,
  `pkg-config`, `wget`.
- Rust `BRIDGE_RUST_VERSION` installieren.
- Flutter `BRIDGE_FLUTTER_VERSION` installieren.
- Danach exakt:

  ```bash
  cargo install cargo-expand --version "$CARGO_EXPAND_VERSION" --locked
  cargo install flutter_rust_bridge_codegen \
    --version "$FLUTTER_RUST_BRIDGE_VERSION" --features "uuid" --locked
  sed -i -e 's/extended_text: 14.0.0/extended_text: 13.0.0/g' \
    flutter/pubspec.yaml
  pushd flutter
  flutter pub get
  popd
  ~/.cargo/bin/flutter_rust_bridge_codegen \
    --rust-input ./src/flutter_ffi.rs \
    --dart-output ./flutter/lib/generated_bridge.dart \
    --c-output ./flutter/macos/Runner/bridge_generated.h
  cp ./flutter/macos/Runner/bridge_generated.h \
    ./flutter/ios/Runner/bridge_generated.h
  ```

- Vor dem Upload mit `test -s` alle fünf unter „Aktueller Zustand“ genannten
  macOS-relevanten Bridge-Dateien prüfen.
- Als internes Artefakt `ongrow-bridge-1.4.9` mit einem Tag
  Aufbewahrung hochladen. `if-no-files-found: error` setzen und exakt diese
  Pfade übertragen:

  ```text
  ./src/bridge_generated.rs
  ./src/bridge_generated.io.rs
  ./flutter/lib/generated_bridge.dart
  ./flutter/lib/generated_bridge.freezed.dart
  ./flutter/macos/Runner/bridge_generated.h
  ```

#### Job `build-macos-arm64`

- `needs: generate-bridge`
- `runs-on: macos-14`
- `timeout-minutes: 180`
- Checkout ebenfalls exakt `SOURCE_SHA`, rekursive Submodule,
  `persist-credentials: false`.
- Direkt danach Commit, Version und Runnerarchitektur prüfen:

  ```bash
  test "$(git rev-parse HEAD)" = "$SOURCE_SHA"
  test "$(uname -m)" = "arm64"
  test "$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)" = "$VERSION"
  ```

- Die Cache-Umgebungsvariablen wie
  `.github/workflows/flutter-build.yml:674-679` mit
  `actions/github-script` exportieren.
- Buildruntime wie Upstream installieren:

  ```bash
  brew install llvm create-dmg
  if ! command -v pkg-config >/dev/null 2>&1; then
    brew install pkg-config
  fi
  ```

- NASM 2.16.03 von der Upstream-verwendeten offiziellen URL installieren,
  aber vor dem Entpacken zusätzlich die Prüfsumme kontrollieren:

  ```bash
  curl -fsSLO \
    https://www.nasm.us/pub/nasm/releasebuilds/2.16.03/macosx/nasm-2.16.03-macosx.zip
  echo "0d29bcd8a5fc617333f4549c7c1f93d1866a4a0915c40359e0a8585bb1a5aa75  nasm-2.16.03-macosx.zip" \
    | shasum -a 256 -c -
  unzip nasm-2.16.03-macosx.zip
  sudo install -m 0755 nasm-2.16.03/nasm /usr/local/bin/nasm
  nasm --version
  ```

- Flutter `FLUTTER_VERSION` installieren. Anschließend exakt den Patch und
  Workaround aus `.github/workflows/flutter-build.yml:746-757` anwenden:

  ```bash
  cd "$(dirname "$(dirname "$(which flutter)")")"
  git apply \
    "$GITHUB_WORKSPACE/.github/patches/flutter_3.24.4_dropdown_menu_enableFilter.diff"
  ```

  In einem neuen Step:

  ```bash
  cd "$(dirname "$(which flutter)")"
  sed -i -e \
    's/_setFramesEnabledState(false);/\/\/_setFramesEnabledState(false);/g' \
    ../packages/flutter/lib/src/scheduler/binding.dart
  grep -n '_setFramesEnabledState(false);' \
    ../packages/flutter/lib/src/scheduler/binding.dart
  ```

  Diese beiden Befehlsblöcke in getrennten Steps ausführen, damit das
  Arbeitsverzeichnis eines Steps nicht in den nächsten übernommen wird.

- Rust `MAC_RUST_VERSION` für `aarch64-apple-darwin` installieren.
- Das Artefakt `ongrow-bridge-1.4.9` ins Repositorywurzelverzeichnis laden.
- vcpkg mit `VCPKG_COMMIT_ID` einrichten und aus `vcpkg.json` installieren.
  Bei Fehlschlag dieselben Logs wie
  `.github/workflows/flutter-build.yml:782-796` ausgeben und mit Exit 1
  abbrechen.
- Vor dem Build die Mindestversion analog Upstream auf 12.3 setzen:

  ```bash
  MIN_MACOS_VERSION="12.3"
  sed -i -e \
    "s/MACOSX_DEPLOYMENT_TARGET\=[0-9]*.[0-9]*/MACOSX_DEPLOYMENT_TARGET=${MIN_MACOS_VERSION}/" \
    build.py
  sed -i -e \
    "s/platform :osx, '.*'/platform :osx, '${MIN_MACOS_VERSION}'/" \
    flutter/macos/Podfile
  sed -i -e \
    "s/osx_minimum_system_version = \"[0-9]*.[0-9]*\"/osx_minimum_system_version = \"${MIN_MACOS_VERSION}\"/" \
    Cargo.toml
  sed -i -e \
    "s/MACOSX_DEPLOYMENT_TARGET = [0-9]*.[0-9]*;/MACOSX_DEPLOYMENT_TARGET = ${MIN_MACOS_VERSION};/" \
    flutter/macos/Runner.xcodeproj/project.pbxproj
  ./build.py --flutter --hwcodec --unix-file-copy-paste --screencapturekit
  ```

- Das App-Bundle und seine Architektur maschinell prüfen:

  ```bash
  RUSTDESK_APP_PATH="flutter/build/macos/Build/Products/Release/RustDesk.app"
  test -d "$RUSTDESK_APP_PATH"
  RUSTDESK_EXECUTABLE="$(
    /usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' \
      "$RUSTDESK_APP_PATH/Contents/Info.plist"
  )"
  test -x "$RUSTDESK_APP_PATH/Contents/MacOS/$RUSTDESK_EXECUTABLE"
  file "$RUSTDESK_APP_PATH/Contents/MacOS/$RUSTDESK_EXECUTABLE" \
    | grep -q "arm64"
  ```

- Danach ausschließlich das unsigned DMG erzeugen:

  ```bash
  RUSTDESK_DMG="rustdesk-${VERSION}-aarch64-unsigned.dmg"
  rm -f "$RUSTDESK_DMG"
  create-dmg \
    --icon "RustDesk.app" 200 190 \
    --hide-extension "RustDesk.app" \
    --window-size 800 400 \
    --app-drop-link 600 185 \
    "$RUSTDESK_DMG" \
    ./flutter/build/macos/Build/Products/Release/RustDesk.app
  hdiutil verify "$RUSTDESK_DMG"
  shasum -a 256 "$RUSTDESK_DMG" > "${RUSTDESK_DMG}.sha256"
  ```

- DMG und `.sha256` als Artefakt
  `rustdesk-1.4.9-unsigned-macos-aarch64` mit sieben Tagen Aufbewahrung
  hochladen. `if-no-files-found: error` setzen.

Für alle Actions exakt die beim Tag 1.4.9 verifizierten Pins verwenden:

| Action | Vollständiger Pin |
|--------|-------------------|
| `actions/checkout` | `34e114876b0b11c390a56381ad16ebd13914f8d5` |
| `actions/github-script` | `d7906e4ad0b1822421a7e6a35d5ca353c962f410` |
| `dtolnay/rust-toolchain` | `e97e2d8cc328f1b50210efc529dca0028893a2d9` |
| `Swatinem/rust-cache` | `e18b497796c12c097a38f9edb9d0641fb99eee32` |
| `subosito/flutter-action` | `1a449444c387b1966244ae4d4f8c696479add0b2` |
| `actions/download-artifact` | `3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c` |
| `lukka/run-vcpkg` | `b1a0dd252f06b9e25b3c022a9a03bd7a427fb6a2` |
| `actions/upload-artifact` | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` |

**Prüfen**:

```bash
ruby -e 'require "yaml"; YAML.parse_file(ARGV.fetch(0)); puts "YAML_OK"' \
  .github/workflows/ongrow-lab-macos-arm64.yml
ruby -e 's=File.binread(ARGV.fetch(0)); abort "NO_FINAL_NEWLINE" unless s.end_with?("\n"); s.each_line.with_index(1){|l,n| abort "TRAILING_WHITESPACE:#{n}" if l.match?(/[ \t]+\n\z/)}; puts "WHITESPACE_OK"' \
  .github/workflows/ongrow-lab-macos-arm64.yml
test "$(rg -c 'workflow_dispatch:' \
  .github/workflows/ongrow-lab-macos-arm64.yml)" -eq 1
test "$(rg -c '6c578292e8ebbbec708b76986ba8c4bc7c509747' \
  .github/workflows/ongrow-lab-macos-arm64.yml)" -ge 1
! rg -n '^[[:space:]]+(push|pull_request|schedule):|secrets\.|softprops/action-gh-release|codesign|notary-submit' \
  .github/workflows/ongrow-lab-macos-arm64.yml
```

Erwartung: `YAML_OK`, `WHITESPACE_OK`; alle Kommandos Exit 0; die letzte Suche
gibt keine Treffer aus.

### Schritt 3: Lokalen Diff kontrollieren

```bash
git diff --no-index -- /dev/null \
  .github/workflows/ongrow-lab-macos-arm64.yml || test "$?" -eq 1
git diff -- plans/README.md
git status --short
```

Manuell, aber anhand harter Grenzen kontrollieren:

- keine Datei außerhalb des Umfangs wurde geändert;
- kein Secretname und kein Secretwert wurde ergänzt;
- keine Signier-, Notarisierungs- oder Release-Action ist vorhanden;
- der einzige Trigger ist `workflow_dispatch`;
- beide Checkouts verwenden den vollständigen `SOURCE_SHA`;
- das finale Artefakt enthält DMG und SHA-256-Datei.

**Prüfen**:

```bash
{
  git diff --name-only -- .
  git ls-files --others --exclude-standard
} | rg -v '^plans/001-github-actions-macos-arm64-baseline\.md$' | sort -u
```

Erwartung vor einem Commit: höchstens

```text
.github/workflows/ongrow-lab-macos-arm64.yml
plans/README.md
```

Unversionierte, bereits vom Advisor erzeugte Plan-Dateien sind erlaubt und
werden durch `git diff --name-only` nicht als Implementierungsänderung
gezählt.

### Schritt 4: Freigabepunkt 1 – Commit, Push und Pull Request

Hier anhalten und dem Betreiber Diff, statische Prüfergebnisse und die
vorgesehenen externen Schritte zeigen. Ohne ausdrückliche Freigabe weder
committen noch pushen noch einen Pull Request anlegen.

Nach Freigabe:

```bash
git add \
  .github/workflows/ongrow-lab-macos-arm64.yml \
  plans/README.md \
  plans/001-github-actions-macos-arm64-baseline.md
git commit -s -m "ci: add macOS arm64 baseline workflow"
git push -u origin lab/001-macos-arm64-baseline
gh pr create \
  --repo ongrowww/rustdesk-lab \
  --base master \
  --head lab/001-macos-arm64-baseline \
  --title "ci: add macOS arm64 baseline workflow" \
  --body "Adds a manual, unsigned RustDesk 1.4.9 Apple Silicon baseline build. No secrets, signing, releases, or automatic triggers."
```

Der Pull Request ist vor dem Merge menschlich zu prüfen. Merge ist ebenfalls
eine externe Änderung und braucht ausdrückliche Freigabe.

**Prüfen**:

```bash
gh pr view \
  --repo ongrowww/rustdesk-lab \
  --json state,baseRefName,headRefName,files,url
```

Erwartung: offener PR, Basis `master`, Head
`lab/001-macos-arm64-baseline`; die Dateiliste enthält nur die Workflowdatei
und `plans/`.

### Schritt 5: Freigabepunkt 2 – Actions kontrolliert aktivieren

Erst nach geprüftem Merge fortfahren. Noch keinen Build starten.

```bash
gh api repos/ongrowww/rustdesk-lab/actions/permissions
gh api repos/ongrowww/rustdesk-lab/actions/permissions/workflow
gh api repos/ongrowww/rustdesk-lab/actions/workflows \
  --jq '.total_count'
```

Erwartung: Actions `enabled: true`, Standardberechtigung `read` und nach der
Registrierung mindestens ein Workflow.

Falls weiterhin `0` Workflows registriert sind, im GitHub-Tab **Actions** die
einmalige Fork-Bestätigung durchführen. Dies ist eine menschliche
Freigabehandlung. Nicht versuchen, sie durch zusätzliche Repositoryänderungen
zu umgehen.

Nach der Registrierung alle vorhandenen Entry-Point-Workflows auflisten:

```bash
gh workflow list --repo ongrowww/rustdesk-lab --all
```

Vor dem ersten Build dem Betreiber die Liste zeigen. Nach ausdrücklicher
Freigabe die übernommenen Upstream-Entry-Points deaktivieren, damit weder
Nightly-Builds noch spätere Pushes oder Tags unbeabsichtigt breite Builds
starten:

```bash
gh workflow disable ci.yml --repo ongrowww/rustdesk-lab
gh workflow disable flutter-ci.yml --repo ongrowww/rustdesk-lab
gh workflow disable flutter-nightly.yml --repo ongrowww/rustdesk-lab
gh workflow disable flutter-tag.yml --repo ongrowww/rustdesk-lab
gh workflow disable fdroid.yml --repo ongrowww/rustdesk-lab
gh workflow disable playground.yml --repo ongrowww/rustdesk-lab
gh workflow disable clear-cache.yml --repo ongrowww/rustdesk-lab
```

Bereits deaktivierte Workflows müssen nicht erneut geändert werden. Den neuen
Workflow aktiv lassen beziehungsweise gezielt aktivieren:

```bash
gh workflow enable ongrow-lab-macos-arm64.yml \
  --repo ongrowww/rustdesk-lab
gh workflow view ongrow-lab-macos-arm64.yml \
  --repo ongrowww/rustdesk-lab
```

Erwartung: neuer Workflow ist `active`; alle oben genannten Entry-Points sind
`disabled_manually`. Wiederverwendbare Workflows dürfen vorhanden bleiben,
weil der neue Workflow sie nicht aufruft.

### Schritt 6: Den Remote-Baseline-Build starten und prüfen

Erst nach ausdrücklicher Freigabe des kosten- beziehungsweise
ressourcenrelevanten Runs:

```bash
gh workflow run ongrow-lab-macos-arm64.yml \
  --repo ongrowww/rustdesk-lab \
  --ref master
RUSTDESK_RUN_ID="$(
  gh run list \
    --repo ongrowww/rustdesk-lab \
    --workflow ongrow-lab-macos-arm64.yml \
    --limit 1 \
    --json databaseId \
    --jq '.[0].databaseId'
)"
test -n "$RUSTDESK_RUN_ID"
gh run watch "$RUSTDESK_RUN_ID" \
  --repo ongrowww/rustdesk-lab \
  --exit-status
```

**Prüfen**:

```bash
gh run view "$RUSTDESK_RUN_ID" \
  --repo ongrowww/rustdesk-lab \
  --json conclusion,event,headBranch,jobs,url
gh api \
  "repos/ongrowww/rustdesk-lab/actions/runs/${RUSTDESK_RUN_ID}/artifacts" \
  --jq '.artifacts[] | [.name, .expired, .size_in_bytes] | @tsv'
```

Erwartung:

- Gesamtergebnis `success`
- Event `workflow_dispatch`
- Branch `master`
- Jobs `generate-bridge` und `build-macos-arm64` erfolgreich
- Artefakt `rustdesk-1.4.9-unsigned-macos-aarch64`
- `expired` ist `false`, Größe ist größer als 0

Optional zur Übergabe herunterladen; das ist kein lokaler Build:

```bash
RUSTDESK_ARTIFACT_DIR="$(mktemp -d)"
gh run download "$RUSTDESK_RUN_ID" \
  --repo ongrowww/rustdesk-lab \
  --name rustdesk-1.4.9-unsigned-macos-aarch64 \
  --dir "$RUSTDESK_ARTIFACT_DIR"
ls -lh "$RUSTDESK_ARTIFACT_DIR"
(
  cd "$RUSTDESK_ARTIFACT_DIR"
  shasum -a 256 -c rustdesk-1.4.9-aarch64-unsigned.dmg.sha256
)
```

Erwartung: DMG und `.sha256` sind vorhanden; Prüfsumme meldet `OK`.
Das Artefakt nicht als kundentauglichen Download veröffentlichen.

### Schritt 7: Planstatus dokumentieren

- Wenn der Workflow lokal fertig, aber Push/Run noch nicht freigegeben ist:
  Status `IN PROGRESS` mit kurzem Hinweis belassen.
- Nur wenn der Remote-Run erfolgreich war und das finale Artefakt geprüft
  wurde: Status in `plans/README.md` auf `DONE` setzen.
- Bei Fehlschlag: Status `BLOCKED` mit Run-URL und knappem Fehlergrund; keine
  fachfremden Änderungen improvisieren.

**Prüfen**:

```bash
rg -n '001.*(TODO|IN PROGRESS|DONE|BLOCKED)' plans/README.md
git diff --check
```

Erwartung: genau eine Statuszeile für Plan 001 und keine
Whitespace-Fehler.

## Testplan

Dieser Plan ergänzt keinen Produktcode und daher keine Rust-/Flutter-Unit-
Tests. Die Workflow-Verifikation deckt folgende Fälle ab:

1. **Syntax**: YAML lässt sich lokal parsen.
2. **Sicherheitsgrenze**: statische Suche findet keine automatischen Trigger,
   Secrets, Signier-, Notarisierungs- oder Releasebefehle.
3. **Reproduzierbarkeit**: beide Jobs brechen ab, wenn Checkout-SHA oder
   Cargo-Version nicht exakt 1.4.9 entsprechen.
4. **Architektur**: der macOS-Job läuft auf `macos-14`, bestätigt `arm64` und
   prüft das erzeugte App-Executable mit `file`.
5. **Paketintegrität**: `hdiutil verify` validiert das DMG; SHA-256 wird im
   Runner erzeugt und nach Download erneut geprüft.
6. **Keine Veröffentlichung**: Ergebnis liegt nur als kurzlebiges
   Actions-Artefakt vor; es gibt weder Release noch Tag.

Strukturelle Vorbilder sind
`.github/workflows/bridge.yml:13-116` und
`.github/workflows/flutter-build.yml:650-832`. Es werden nur die explizit im
Plan genannten Bridge- und ARM64-Unsigned-Schritte übernommen.

## Fertigkriterien

Alle Punkte müssen erfüllt sein:

- [ ] `.github/workflows/ongrow-lab-macos-arm64.yml` ist die einzige neue
      Implementierungsdatei.
- [ ] YAML-Parser und `git diff --check` enden mit Exit 0.
- [ ] Der Workflow hat ausschließlich `workflow_dispatch`.
- [ ] `permissions: contents: read` ist gesetzt.
- [ ] Kein `secrets`, `codesign`, `notary-submit` oder Release-Upload ist
      enthalten.
- [ ] Beide Jobs checken exakt
      `6c578292e8ebbbec708b76986ba8c4bc7c509747` aus.
- [ ] Upstream-Entry-Point-Workflows sind vor dem ersten Run kontrolliert
      deaktiviert.
- [ ] Der GitHub-Run endet mit `success`.
- [ ] Der Run bestätigt ein ARM64-App-Executable und ein mit
      `hdiutil verify` gültiges DMG.
- [ ] Das finale Artefakt
      `rustdesk-1.4.9-unsigned-macos-aarch64` existiert, ist nicht abgelaufen
      und hat eine Größe größer als 0.
- [ ] Die heruntergeladene SHA-256-Datei validiert das DMG.
- [ ] Kein GitHub Release und kein neuer Tag wurde angelegt.
- [ ] `plans/README.md` wurde passend auf `DONE` oder `BLOCKED` aktualisiert.

## STOP-Bedingungen

Anhalten und berichten, nicht improvisieren, wenn:

- die Drift-Prüfung eine Änderung der Bridge- oder macOS-Upstream-Buildlogik
  zeigt;
- Tag `1.4.9` nicht exakt auf
  `6c578292e8ebbbec708b76986ba8c4bc7c509747` zeigt;
- fremde oder unerklärte Arbeitskopieänderungen mit dem Umfang kollidieren;
- die Umsetzung eine Datei außerhalb des definierten Umfangs zu benötigen
  scheint;
- ein Secret, Apple-Zertifikat oder kostenpflichtiger/larger Runner nötig
  erscheint;
- GitHub den Workflow nach Merge und menschlicher Actions-Aktivierung nicht
  registriert;
- ein übernommener Upstream-Workflow nicht kontrolliert deaktiviert werden
  kann;
- die Architekturprüfung nicht `arm64` ergibt;
- ein Verifikationsschritt nach einem nachvollziehbaren Korrekturversuch ein
  zweites Mal fehlschlägt;
- der Build nur durch Abschwächung von Gatekeeper, SIP, Signaturprüfung oder
  anderen Sicherheitsmechanismen ausführbar wäre;
- Push, PR, Merge, Workflow-Aktivierung oder Run nicht ausdrücklich
  freigegeben wurden.

## Wartungshinweise

- Der Workflow ist absichtlich eine schmale, kopierte Baseline statt eines
  Aufrufs des breiten Upstream-Workflows. Bei einem späteren RustDesk-Update
  müssen Action-Pins, Rust/Flutter/vcpkg-Versionen, Patches, Bridge-Generator
  und macOS-Buildschritte gemeinsam neu abgeglichen werden.
- Reviewer sollten besonders prüfen, dass die zwei Checkouts weiterhin auf
  demselben vollständigen Commit stehen und keine Release- oder Secretlogik
  hineingeraten ist.
- Das öffentliche Repository macht Workflowdatei und Logs sichtbar. Niemals
  Passwörter, Kunden-IDs, Server-Private-Keys oder Apple-Zertifikate in YAML,
  Plan oder Logs schreiben.
- Ein späterer Signierplan benötigt Apple Developer ID und Notarisierung und
  muss die Distribution gesondert absichern. Bis dahin bleibt dieses DMG ein
  internes Testartefakt.
- Erst nach erfolgreicher Baseline folgen getrennte Pläne für Branding,
  eigene Serverwerte, kontrollierte Upstream-Synchronisation, weitere
  Plattformen und gegebenenfalls Blacksmith.
