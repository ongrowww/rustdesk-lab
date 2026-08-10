# Plan 002: Serverbestätigte Geräteidentität für das Control Plane bereitstellen

> **Executor-Anweisung**: Diesen Plan vollständig und Schritt für Schritt
> ausführen. Nach jedem Schritt die Prüfung ausführen und das erwartete
> Ergebnis bestätigen. Bei einer STOP-Bedingung anhalten und berichten, nicht
> improvisieren. Nach Abschluss die Statuszeile in `plans/README.md`
> aktualisieren, sofern ein Reviewer den Index nicht selbst pflegt.
>
> **Drift-Prüfung (zuerst ausführen)**:
>
> ```bash
> git -C /Users/schrobo/Developer/ongrow/rustdesk-lab diff --stat \
>   29723655..HEAD -- \
>   libs/hbb_common/protos/rendezvous.proto \
>   src/ui_interface.rs \
>   src/flutter_ffi.rs \
>   flutter/test
> git -C /Users/schrobo/Developer/ongrow/rustdesk-server-lab diff --stat \
>   df5b912..HEAD -- \
>   libs/hbb_common/protos/rendezvous.proto \
>   src/common.rs src/peer.rs src/rendezvous_server.rs
> ```
>
> Wenn sich eine Datei im Umfang geändert hat, die Ausschnitte unter
> „Aktueller Zustand“ mit dem Live-Code vergleichen. Bei semantischer
> Abweichung anhalten.

## Status

- **Priorität**: P1
- **Aufwand**: L
- **Risiko**: HIGH
- **Abhängig von**: Plan 001 ist DONE; keine offene technische Abhängigkeit
- **Kategorie**: security / direction
- **Geplant bei**: Client `29723655`, Server `df5b912`, 2026-07-30

## Warum das wichtig ist

Ein späteres Admin-System darf eine vom Client behauptete `OG-xxxx`-ID nicht
ungeprüft als Geräteidentität übernehmen. Der OSS-Server kennt die tatsächliche
Zuordnung von ID, UUID und Ed25519-Geräteschlüssel. Dieser Plan lässt `hbbs`
deshalb nach einem erneut signierten Besitznachweis eine kurzlebige,
server-signierte Attestierung ausstellen. Das spätere Control Plane kann damit
Geräte aufnehmen, ohne die RustDesk-SQLite-Datenbank zu teilen und ohne einen
globalen Enrollment-Token in den Client einzubauen.

## Aktueller Zustand

- Client und Server besitzen getrennte Thin Forks:
  - `/Users/schrobo/Developer/ongrow/rustdesk-lab`
  - `/Users/schrobo/Developer/ongrow/rustdesk-server-lab`
- `libs/hbb_common` ist in beiden Forks ein Git-Submodul auf unterschiedlichen
  Upstream-Ständen. Diese während der Ausführung festgestellte Grenze erfordert
  den zusätzlichen öffentlichen Thin Fork `ongrowww/hbb_common`; Client und
  Server pinnen dort getrennte Branches.
- Beide Protobuf-Dateien enthielten zu Planbeginn `RegisterPk` und
  `RegisterPkResponse`, aber noch keine Nonce oder Geräteattestierung:

  ```proto
  // rustdesk-lab/libs/hbb_common/protos/rendezvous.proto:95-115
  message RegisterPk {
    string id = 1;
    bytes uuid = 2;
    bytes pk = 3;
    string old_id = 4;
    bool no_register_device = 5;
  }

  message RegisterPkResponse {
    Result result = 1;
    int32 keep_alive = 2;
  }
  ```

- Der Client signiert aktuell nur alte ID, neue ID und UUID mit Kontext
  `ongrow-rustdesk-custom-id-v1`
  (`rustdesk-lab/src/ui_interface.rs:1492-1520`).
- Der Server prüft diesen Besitznachweis gegen den bereits gespeicherten
  Geräteschlüssel (`rustdesk-server-lab/src/common.rs:14-50`) und führt den
  Wechsel atomar aus (`src/peer.rs:135-190`).
- Der TCP-Handler antwortet derzeit nur mit einem Ergebniscode
  (`src/rendezvous_server.rs:548-568`).
- `RendezvousServer::Inner` hält bereits den privaten Ed25519-Serverschlüssel
  (`src/rendezvous_server.rs:72-90`). Derselbe Schlüssel signiert schon
  ID/PK-Antworten (`src/rendezvous_server.rs:1215-1226`).
- Der Thin-Fork-Grundsatz aus `rustdesk-server-lab/ONGROW.md` bleibt bestehen:
  Änderungen klein halten, `master` folgt Upstream, OnGROW-Funktionen liegen
  auf Feature-Branches.

## Festgelegtes Protokoll

Diese Felder in **beiden** `rendezvous.proto`-Kopien mit identischen
Feldnummern ergänzen; bestehende Nummern niemals ändern:

```proto
message RegisterPk {
  // bestehende Felder 1-5 bleiben unverändert
  bytes ongrow_attestation_nonce = 6;
}

message OnGrowDeviceAttestation {
  string id = 1;
  bytes device_pk = 2;
  bytes uuid_sha256 = 3;
  int64 issued_at = 4;
  int64 expires_at = 5;
  bytes nonce = 6;
  bytes signed_payload = 7;
}

message RegisterPkResponse {
  // bestehende Felder 1-2 bleiben unverändert
  OnGrowDeviceAttestation ongrow_device_attestation = 3;
}
```

Regeln:

- Nonce exakt 32 Bytes aus einer CSPRNG.
- Attestierung nur bei `Result::OK`, gültiger `OG-xxxx`-ID, erfolgreichem
  Gerätebesitznachweis und vorhandenem privaten Serverschlüssel.
- Gültigkeit exakt fünf Minuten; tolerierte Uhrabweichung wird erst im Control
  Plane geprüft.
- UUID niemals roh ausgeben; ausschließlich SHA-256.
- `signed_payload` ist `sodiumoxide::crypto::sign::sign(canonical_payload, sk)`.
- Kanonischer Payload-Kontext:
  `ongrow-rustdesk-device-attestation-v1`.
- Danach folgende Felder jeweils als `u32` Big-Endian-Länge plus Bytes:
  `id`, `device_pk`, `uuid_sha256`, `issued_at` als 8 Big-Endian-Bytes,
  `expires_at` als 8 Big-Endian-Bytes und `nonce`.
- Der Clientbesitznachweis mit Nonce verwendet Kontext
  `ongrow-rustdesk-custom-id-v2` und bindet nach alter ID, neuer ID und UUID
  zusätzlich die Nonce. V1 bleibt nur für bestehende ID-Änderungen ohne
  Attestierungsanforderung kompatibel.
- Logausgaben dürfen weder Attestierung, Signatur, UUID noch Nonce enthalten.

## Benötigte Befehle

| Zweck | Befehl | Erwartung |
|---|---|---|
| Server formatieren | `cargo fmt --package hbbs -- --check` | Exit 0 |
| Server testen | `cargo test --locked` | alle Tests grün |
| Server bauen | `cargo build --release --locked --bin hbbs --bin hbbr` | Exit 0 |
| Client Dart-Tests | `flutter test --no-pub test/ongrow_device_attestation_test.dart` | alle Tests grün |
| Client analysieren | `flutter analyze --no-pub lib test/ongrow_device_attestation_test.dart` | keine neuen Fehler |
| Diff prüfen | `git diff --check` | keine Ausgabe, Exit 0 |

Für den Client ist der in
`.github/workflows/ongrow-lab-macos-arm64.yml` gepinnte Flutter-/Rust-Workflow
die maßgebliche vollständige Buildprüfung.

## Umfang

**Client-Fork, im Umfang**:

- `.gitmodules`
- `libs/hbb_common/protos/rendezvous.proto`
- `src/ui_interface.rs`
- `src/flutter_ffi.rs`
- `flutter/lib/common/ongrow_device_attestation.dart` (neu)
- `flutter/lib/web/bridge.dart`
- `flutter/test/ongrow_device_attestation_test.dart` (neu)
- `.github/workflows/ongrow-lab-macos-arm64.yml`, nur falls ein neuer
  gezielter Testschritt erforderlich ist

**Server-Fork, im Umfang**:

- `.gitmodules`
- `libs/hbb_common/protos/rendezvous.proto`
- `src/common.rs`
- `src/peer.rs`
- `src/rendezvous_server.rs`
- `ONGROW.md`
- `.github/workflows/ongrow-custom-id-ci.yml`, nur für gezielte Prüfschritte

**Außerhalb des Umfangs**:

- Control-Plane-HTTP-API und Datenbank
- Admin-UI
- Festpasswort oder unbeaufsichtigter Zugriff
- Änderung der ID-Vergaberegeln
- neue Serverports oder öffentliche HTTP-Endpunkte
- Produktionsdeployment, Commit, Push oder PR ohne separate Freigabe
- Lesen oder Dokumentieren konkreter Serveradressen, Geräte-IDs oder Schlüssel

## Git-Arbeitsweise

- Clientbranch: `feature/004-device-attestation`
- Serverbranch: `feature/002-device-attestation`
- Gemeinsamer Submodule-Fork: `ongrowww/hbb_common`
  - Clientbasis: `ongrow/client-device-attestation`
  - Serverbasis: `ongrow/server-device-attestation`
- Je Fork ein kleiner, unabhängig testbarer Conventional Commit mit DCO:
  - Client: `feat: request server-attested device identity`
  - Server: `feat: attest OnGROW device identities`
- Keine Branches aus einem schmutzigen Arbeitsbaum erstellen.
- Bestehende unversionierte `plans/`-Dateien erhalten.
- Nicht pushen und keinen PR öffnen, bevor der Betreiber Diff und Tests
  freigegeben hat.

## Schritte

### Schritt 1: Gemeinsamen Protobuf-Vertrag ergänzen

Die oben festgelegten Felder und `OnGrowDeviceAttestation` auf getrennten
Branches des OnGROW-`hbb_common`-Forks identisch ergänzen. Beide
Parent-Repositories über `.gitmodules` und ihren Gitlink reproduzierbar auf
den jeweiligen Commit pinnen. Mit einer kleinen read-only Vergleichsprüfung
sicherstellen, dass die neuen Message-Blöcke bytegleich sind; die restlichen,
versionsbedingt unterschiedlichen Protokolldateien nicht angleichen.

**Prüfen**:

```bash
rg -n 'ongrow_attestation_nonce|OnGrowDeviceAttestation|ongrow_device_attestation' \
  /Users/schrobo/Developer/ongrow/rustdesk-lab/libs/hbb_common/protos/rendezvous.proto \
  /Users/schrobo/Developer/ongrow/rustdesk-server-lab/libs/hbb_common/protos/rendezvous.proto
```

Erwartung: je Symbol genau ein Treffer pro Fork; vorhandene Feldnummern sind
unverändert.

### Schritt 2: Kanonische Payloads und Testvektoren implementieren

Im Server `src/common.rs` Funktionen für:

- V2-Besitznachweis mit Nonce,
- SHA-256 der UUID,
- kanonischen Attestierungs-Payload,
- Ausstellung und Prüfung einer Attestierung

ergänzen. Bestehende V1-Prüfung für noncefreie Requests erhalten.

Im Client dieselbe V2-Payloadlogik in `src/ui_interface.rs` implementieren.
Einen festen, nicht geheimen Testvektor mit Testschlüsseln, IDs, UUID,
Zeitstempeln und Nonce in beiden Forks verwenden. Die Tests müssen beweisen:

- Server und Client erzeugen denselben Payload;
- Änderung eines Feldes macht die Signatur ungültig;
- falsche Noncelänge wird abgelehnt;
- UUID erscheint nicht roh in der Attestierung;
- Ablaufzeit liegt exakt 300 Sekunden nach Ausstellung.

**Prüfen**:

```bash
git -C /Users/schrobo/Developer/ongrow/rustdesk-server-lab diff --check
git -C /Users/schrobo/Developer/ongrow/rustdesk-lab diff --check
```

Erwartung: beide Exit 0.

### Schritt 3: Serverantwort an atomare Identitätsprüfung koppeln

`PeerMap::change_id` so strukturieren, dass der Handler bei Erfolg einen
read-only Identitätssnapshot aus `id`, UUID und gespeichertem Public Key
erhält. Kein zweites ungebundenes Lookup nach der Änderung verwenden.

Im TCP-Handler:

1. V1-Anfrage ohne Nonce wie bisher behandeln und keine Attestierung ausgeben.
2. V2-Anfrage mit 32-Byte-Nonce prüfen.
3. Nur nach `OK` und nur mit `self.inner.sk = Some(...)` eine Attestierung
   erzeugen.
4. Bei fehlendem privaten Serverschlüssel keine Attestierung ausgeben und
   einen klaren, nicht sensitiven Fehlerstatus zurückgeben. Keinesfalls
   unsigned antworten.

Stock-Clients müssen unbekannte Felder weiterhin ignorieren können.

**Prüfen**:

```bash
cd /Users/schrobo/Developer/ongrow/rustdesk-server-lab
cargo test --locked
```

Erwartung: bestehende und neue Tests grün.

### Schritt 4: Dedizierte Clientfunktion zum Anfordern implementieren

Eine neue Rust-Funktion hinzufügen, die für die aktuell gültige `OG-xxxx`-ID:

1. UUID und Geräteschlüsselpaar über die bestehenden Config-/IPC-Wege lädt,
2. 32 zufällige Noncebytes mit OS-CSPRNG erzeugt,
3. einen V2-Besitznachweis für `old_id == id` signiert,
4. genau einen konfigurierten primären Rendezvous-Server anspricht,
5. Ergebnis, Attestierung und Nonce validiert,
6. die protobuf-serialisierte Attestierung Base64-kodiert zurückgibt.

Die Funktion darf die Attestierung nicht persistent speichern und nicht den
globalen `ASYNC_JOB_STATUS` der ID-Änderung verwenden. Über
`src/flutter_ffi.rs` eine dedizierte FFI-Funktion bereitstellen; noch keinen
automatischen HTTP-Upload implementieren.

**Prüfen**:

```bash
cd /Users/schrobo/Developer/ongrow/rustdesk-lab/flutter
flutter test --no-pub test/ongrow_device_attestation_test.dart
flutter analyze --no-pub \
  lib/desktop/pages/ongrow_support_home.dart \
  test/ongrow_device_attestation_test.dart
```

Erwartung: alle Tests grün, Analyse ohne neue Fehler.

### Schritt 5: Abwärtskompatibilität und Fehlerpfade testen

Servertests ergänzen für:

- Stock-/V1-Request: unverändertes Ergebnis, keine Attestierung;
- V2 mit falscher Nonce, UUID, ID, PK oder Signatur: keine Attestierung;
- V2 mit `old_id == id`: keine DB-Änderung, gültige Attestierung;
- V2 nach erfolgreicher ID-Änderung: Attestierung enthält neue ID;
- fehlender Serverschlüssel: niemals unsigned;
- parallel wiederholte Requests: jeweils gültige, noncegebundene Antwort,
  keine DB-Kollision.

Clienttests ergänzen für:

- falscher Serverpublickey;
- Antwortnonce ungleich Requestnonce;
- abgelaufene oder zukünftig datierte Antwort;
- fehlendes Attestierungsfeld;
- `NOT_SUPPORT` von altem Server.

**Prüfen**: beide Fork-Testkommandos aus „Benötigte Befehle“ → grün.

### Schritt 6: CI bauen, aber noch nicht deployen

Nach lokaler Prüfung und ausdrücklicher Freigabe je Fork committen/pushen,
Server-CI und macOS-ARM64-Workflow ausführen. Artefakte und SHA-256 wie in den
bestehenden Workflows prüfen. Noch kein VPS-Deployment und keine lokale
App-Installation.

**Prüfen**:

```bash
gh run view <SERVER_RUN_ID> --repo ongrowww/rustdesk-server-lab \
  --json conclusion,headSha,jobs,url
gh run view <CLIENT_RUN_ID> --repo ongrowww/rustdesk-lab \
  --json conclusion,headSha,jobs,url
```

Erwartung: beide `conclusion: success`, Head-SHAs entsprechen den
freigegebenen Commits.

## Testplan

- Unit-Tests für beide kanonischen Payloadversionen.
- Gemeinsamer fester Interoperabilitäts-Testvektor in beiden Forks.
- Servertests für Erfolg, Manipulation, Ablauf, Nonce und fehlenden
  Serverschlüssel.
- Clienttests für Antwortvalidierung und alte Server.
- Bestehende Custom-ID-Tests bleiben unverändert grün.
- Vollständige Remote-Builds beider Forks als Integrationsgate.

## Fertigkriterien

- [ ] Beide Protobuf-Dateien verwenden identische neue Feldnummern.
- [ ] V1-ID-Wechsel funktioniert unverändert.
- [ ] V2 bindet eine 32-Byte-Nonce in den Gerätebesitznachweis ein.
- [ ] UUID verlässt den Server nur als SHA-256.
- [ ] Attestierungen sind exakt fünf Minuten gültig und server-signiert.
- [ ] Ohne privaten Serverschlüssel wird nie attestiert.
- [ ] Client validiert Serverkey, Signatur, Nonce, ID und Zeitfenster.
- [ ] Keine Attestierung, Signatur, UUID oder Nonce wird geloggt.
- [ ] Server- und Clienttests sowie beide CI-Builds sind grün.
- [ ] Kein Control Plane, Festpasswort oder Deployment wurde vorgezogen.
- [ ] Planstatus im Index ist aktualisiert.

## STOP-Bedingungen

Anhalten und berichten, wenn:

- Feldnummern mit Upstream-Feldern kollidieren;
- Client- und Server-Protobuf nicht kompatibel generiert werden;
- der aktive Server keinen privaten Ed25519-Schlüssel besitzt;
- die Attestierung nur durch Ausgabe der rohen UUID möglich scheint;
- ein globaler/shared Enrollment-Token vorgeschlagen wird;
- Tests nur durch Abschalten bestehender Signatur- oder Formatprüfungen grün
  werden;
- eine konkrete Serveradresse oder ein geheimer Schlüssel in Code, Plan,
  Workflow oder Log geschrieben werden müsste;
- fremde Arbeitsbaumänderungen mit dem Umfang kollidieren;
- Commit, Push, Workflow-Run oder Deployment nicht ausdrücklich freigegeben
  ist.

## Wartungshinweise

- Bei späteren Upstream-Protobuf-Änderungen müssen die reservierten
  Feldnummern erneut geprüft werden.
- Der Serverschlüssel wird für einen neuen, domain-separierten Signaturzweck
  wiederverwendet. Reviewer müssen die Kontexttrennung und Payloadgleichheit
  besonders prüfen.
- Die Forks stehen unter AGPL-3.0. Vor einem externen Rollout muss Plan 008
  die Quellcodebereitstellung und Lizenzhinweise prüfen.
- Attestierungen sind keine Sitzungen und enthalten kein Zugriffsrecht. Sie
  beweisen ausschließlich die aktuelle Zuordnung von OnGROW-ID und
  Geräteschlüssel.
