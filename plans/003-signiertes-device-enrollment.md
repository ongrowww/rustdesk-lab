# Plan 003: Signiertes Geräte-Enrollment und Heartbeats im Control Plane aufbauen

> **Executor-Anweisung**: Diesen Plan vollständig ausführen. Alle
> Verifikationsgates sind verbindlich. Bei einer STOP-Bedingung nicht
> improvisieren. Keine Remote-Repositories, Deployments oder Secrets ohne
> ausdrückliche Betreiberfreigabe anlegen.
>
> **Drift-Prüfung (zuerst ausführen)**:
>
> ```bash
> git -C /Users/schrobo/Developer/ongrow/rustdesk-lab diff --stat \
>   29723655..HEAD -- \
>   src/ui_interface.rs src/flutter_ffi.rs \
>   flutter/lib/desktop/pages/ongrow_support_home.dart \
>   libs/hbb_common/protos/rendezvous.proto
> git -C /Users/schrobo/Developer/ongrow/rustdesk-server-lab diff --stat \
>   df5b912..HEAD -- libs/hbb_common/protos/rendezvous.proto
> ```
>
> Erwartete Voraussetzung: Plan 002 ist DONE und die Live-Dateien enthalten
> `OnGrowDeviceAttestation`. Wenn nicht, anhalten.

## Status

- **Priorität**: P1
- **Aufwand**: L
- **Risiko**: HIGH
- **Abhängig von**: `plans/002-serverbestaetigte-geraeteidentitaet.md`
- **Kategorie**: security / direction
- **Geplant bei**: Client `29723655`, Server `df5b912`, 2026-07-30

## Warum das wichtig ist

Der RustDesk-OSS-Server speichert technische Peers, bietet aber kein
mandantenfähiges, signiertes Geräteverzeichnis. Dieser Plan erstellt ein
separates OnGROW Control Plane und lässt den gebrandeten Client ausschließlich
mit server-attestierter Identität enrollen. Danach existiert erstmals eine
verlässliche, API-seitig abfragbare Liste aktiver OnGROW-Geräte — noch ohne
Kundenzuordnung, Admin-UI oder unbeaufsichtigte Zugangsdaten.

## Aktueller Zustand und Zielarchitektur

- `rustdesk-server-lab/src/database.rs` speichert `guid`, `id`, `uuid`, `pk`
  und technisches `info`, besitzt jedoch keine Admin-API.
- Der gebrandete Client besitzt bereits:
  - automatische `OG-xxxx`-Vergabe in
    `flutter/lib/desktop/pages/ongrow_support_home.dart:119-186`;
  - Geräte-Metadaten über
    `src/ui_interface.rs:1328-1339` (`os`, `type`, `name`);
  - HTTP- und Ed25519-Grundfunktionen in Rust.
- Plan 002 liefert eine kurzlebige, `hbbs`-signierte Attestierung.
- Das neue Control Plane wird bewusst **nicht** in den RustDesk-Server-Fork
  eingebaut. Es entsteht lokal als separates Repository:

  ```text
  /Users/schrobo/Developer/ongrow/ongrow-support-control/
    cmd/control-api/
    internal/
      api/
      application/
      attestation/
      domain/
      persistence/
    migrations/
    openapi/
    deploy/
    Dockerfile
    go.mod
    README.md
  ```

- Sprache: Go, passend zum bereits gepflegten OnGROW-Go-Code. Ports/Adapter,
  explizite Zustände und sichere Fehlerkategorien aus
  `macbook-company-setup/tools/update-center/internal/application/app.go`
  dienen als Stilvorbild. Keine rohe interne Fehlermeldung an Clients.
- Persistenz für den Pilot: SQLite mit WAL, Foreign Keys, Busy Timeout und
  versionierten eingebetteten Migrationen. Diese Wahl passt zum kleinen
  Gerätebestand; ein Wechsel auf PostgreSQL ist außerhalb dieses Plans.

## Enrollment-Vertrag

Öffentliche Endpunkte unter `/v1/device/`:

- `POST /v1/device/enroll`
- `POST /v1/device/heartbeat`

Enrollment-Body, Größenlimits verbindlich:

```json
{
  "device_id": "OG-0000",
  "device_name": "max. 120 UTF-8 bytes",
  "os": "macos|windows|linux",
  "app_version": "max. 40 ASCII bytes",
  "permissions": {
    "screen_recording": true,
    "accessibility": true,
    "input_monitoring": true,
    "audio_recording": false,
    "network": true
  },
  "attestation": "<base64 protobuf, max. 4 KiB>",
  "request_timestamp": 0,
  "request_nonce": "<base64 32 bytes>",
  "device_signature": "<base64 signed canonical request, max. 8 KiB>"
}
```

Sicherheitsregeln:

- Control Plane prüft Attestierung mit dem konfigurierten öffentlichen
  `hbbs`-Schlüssel; niemals mit einem vom Request gelieferten Trust Anchor.
- Attestierungs-ID und Public Key müssen zum Request passen.
- Zeitfenster: Attestierung gültig, Request maximal 120 Sekunden alt,
  tolerierte Uhrabweichung maximal 60 Sekunden.
- Requestnonce ist 32 Bytes und pro Gerätekey eindeutig. Replay-Hashes werden
  mindestens bis Attestierungsablauf gespeichert.
- Geräte-Signatur verwendet Kontext
  `ongrow-control-device-enrollment-v1` und einen dokumentierten
  längenpräfigierten Payload. JSON selbst wird nicht signiert.
- Primäridentität ist der SHA-256-Fingerprint des Ed25519-Geräteschlüssels.
  `device_id` ist eindeutig, aber veränderbar.
- UUID-Digest wird gespeichert; rohe UUID wird weder akzeptiert noch geloggt.
- Enrollment ist idempotent. Ein anderer Key darf eine bestehende ID nicht
  übernehmen.
- Heartbeat verwendet Kontext `ongrow-control-device-heartbeat-v1`, dieselbe
  Nonce-/Zeitprüfung und den bereits enrollten Geräteschlüssel.
- Bodylimit 16 KiB; keine unbekannten JSON-Felder.
- Antworten enthalten sichere Fehlercodes, keine Schlüssel, Signaturen,
  Nonces, SQL- oder Stackdetails.

## Benötigte Befehle

### Neues Control Plane

| Zweck | Befehl | Erwartung |
|---|---|---|
| Formatcheck | `gofmt -l .` | keine Ausgabe |
| Tests | `go test ./...` | alle grün |
| Race-Tests | `go test -race ./...` | alle grün |
| Vet | `go vet ./...` | Exit 0 |
| Build | `go build ./cmd/control-api` | Exit 0 |
| Image | `docker build -t ongrow-support-control:test .` | Exit 0 |
| Compose | `docker compose -f deploy/compose.yml config` | Exit 0 |

`go.mod` muss die auf dem Arbeitsrechner vorhandene, unterstützte Go-Version
verwenden und alle Abhängigkeiten exakt über `go.sum` sperren.

### Client

| Zweck | Befehl | Erwartung |
|---|---|---|
| Rusttests | `cargo test --lib --features flutter ongrow_control` | grün |
| Fluttertest | `flutter test --no-pub test/ongrow_device_enrollment_test.dart` | grün |
| Analyse | `flutter analyze --no-pub lib/desktop/pages/ongrow_support_home.dart test/ongrow_device_enrollment_test.dart` | keine neuen Fehler |

## Umfang

**Im neuen Control-Plane-Repo im Umfang**:

- oben genannte Go-Struktur
- SQLite-Migrationen für `devices`, `device_nonces`, `schema_migrations`
- OpenAPI-Vertrag
- Unit-/HTTP-/Persistence-Tests
- gehärtetes Multi-Stage-Dockerfile
- Compose-Beispiel ohne Secretwerte
- Health-/Readiness-Endpunkte

**Im Client-Fork im Umfang**:

- `src/ui_interface.rs`
- `src/flutter_ffi.rs`
- `flutter/lib/desktop/pages/ongrow_support_home.dart`
- neue kleine Rustmodule unter `src/ongrow_control/`, falls zur Trennung nötig
- `flutter/test/ongrow_device_enrollment_test.dart`
- Branding-/Buildkonfiguration ausschließlich für eine öffentliche
  Control-Plane-Basis-URL als GitHub Repository Variable, niemals als Secret

**Außerhalb des Umfangs**:

- Admin-Login oder Browser-UI
- Kunden/Organisationen und manuelle Zuordnung
- Festpasswörter, Secret-Speicher oder Zugriffserteilung
- Anzeige oder Fernsteuerung eines Geräts
- Scraping/Teilen der `hbbs`-SQLite-Datenbank
- öffentliche GitHub-Repositories oder VPS-Deployment ohne Freigabe
- konkrete Domains, Netzwerkadressen, IDs oder Schlüssel in versionierten
  Dateien

## Git-Arbeitsweise

- Neues lokales Repo: `ongrow-support-control`, Branch
  `feature/001-device-enrollment`.
- Clientbranch: `feature/005-device-enrollment`.
- Conventional Commits mit DCO, logisch getrennt:
  - `feat: add signed device enrollment API`
  - `feat: enroll OnGROW support devices`
- Das neue GitHub-Repository zunächst nicht automatisch erstellen. Nach
  Betreiberfreigabe standardmäßig privat anlegen; eine spätere
  Open-Source-Entscheidung separat treffen.
- Kein Push, Deployment oder DNS-/Traefik-Eingriff ohne Freigabe.

## Schritte

### Schritt 1: Lokales Control-Plane-Repository und Qualitätsbaseline anlegen

Verzeichnisstruktur, Go-Modul, `AGENTS.md`, `.gitignore`, `README.md`,
Dockerfile und CI anlegen. `AGENTS.md` muss enthalten:

- keine Secretwerte lesen/loggen/committen;
- sichere Fehlercodes;
- `go test -race ./...`, `go vet ./...`, `gofmt -l .`;
- Migrationen vor Schemaänderungen;
- keine Deployments/Commits/Pushes ohne Freigabe.

CI nur für Tests/Build, minimale `contents: read`-Berechtigung, Actions per
vollständigem SHA pinnen.

**Prüfen**: alle fünf Control-Plane-Kommandos außer Compose → Exit 0.

### Schritt 2: Domainmodell und SQLite-Migrationen implementieren

`devices` mindestens mit folgenden Feldern:

- `device_key_fingerprint` BLOB/TEXT PRIMARY KEY
- `device_public_key` BLOB NOT NULL
- `device_id` TEXT UNIQUE NOT NULL
- `uuid_sha256` BLOB NOT NULL
- `device_name`, `os`, `app_version`
- einzelne boolesche Permission-Spalten
- `attested_at`, `last_seen_at`, `created_at`, `updated_at`

`device_nonces` speichert ausschließlich Hash, Gerätefingerprint und Ablauf.
Keine Attestierung oder Requestsignatur dauerhaft speichern.

Migrationen müssen in Transaktionen laufen. Tests decken frische DB,
Wiederholung, Unique Constraints und Migration einer bereits aktuellen DB ab.

**Prüfen**: `go test ./internal/persistence/...` → grün.

### Schritt 3: Attestierung und Gerätesignatur verifizieren

Im Package `internal/attestation`:

- Base64 strikt dekodieren;
- Protobuf parsen;
- Serverschlüssel ausschließlich aus einer validierten Konfigurationsdatei
  oder Docker-Secret-Pfad laden;
- kanonischen Payload exakt wie Plan 002 rekonstruieren;
- `signed_payload` prüfen;
- Zeit, Nonce, ID, Key und UUID-Digest validieren.

Den festen Interoperabilitäts-Testvektor aus Plan 002 übernehmen. Keine
RustDesk-Quellen kopieren; nur den dokumentierten Vertrag implementieren.

**Prüfen**: `go test ./internal/attestation/...` → grün, inklusive
Manipulationsfälle.

### Schritt 4: Enrollment- und Heartbeat-Endpunkte implementieren

HTTP-Schichten trennen:

- `api`: Decode, Limits, sichere Statuscodes
- `application`: Validierung/Use Cases
- `persistence`: atomare Upserts und Replay-Schutz

Statuscodes:

- `201` erstes Enrollment
- `200` idempotentes Re-Enrollment/Heartbeat
- `400` Format/Größe
- `401` Signatur/Attestierung
- `409` ID gehört anderem Key
- `429` Rate Limit
- `503` temporäre Persistenzstörung

Rate Limit pro IP und Gerätefingerprint; Reverse-Proxy-IP nur aus einer
expliziten Trusted-Proxy-Liste übernehmen.

**Prüfen**: `go test -race ./internal/api/... ./internal/application/...`
→ grün.

### Schritt 5: Clientseitige signierte Enrollment-Schleife ergänzen

Enrollment erst starten, wenn:

- Appname exakt `OnGROW Support Desk`,
- Verbindung online,
- ID entspricht `^OG-[0-9]{4}$`,
- Control-Plane-URL ist gültiges HTTPS,
- Plan-002-Attestierung erfolgreich.

Metadaten aus bestehenden APIs verwenden. Für Heartbeats:

- Intervall fünf Minuten;
- exponentieller Backoff mit Jitter bis maximal eine Stunde;
- bei Resume sofort, aber gegen parallele Läufe geschützt;
- keine Requests während Offline-Status;
- keine Payloaddaten loggen.

Eine lokale, nicht sensitive Enrollment-State-Marke darf letzten Erfolg und
Retryzeit speichern; niemals Signatur, Attestierung oder UUID.

**Prüfen**: gezielte Rust-/Fluttertests → grün.

### Schritt 6: Health, Backupfähigkeit und Containergrenzen ergänzen

- `/healthz`: Prozess lebt, keine DB-Details.
- `/readyz`: DB/Migration bereit, keine sensitiven Details.
- Container läuft non-root, read-only root filesystem, tmpfs für `/tmp`,
  persistentes Volume nur für DB.
- Compose-Beispiel bindet API nur an einen internen Port; TLS terminiert an
  vorhandenem Reverse Proxy.
- Dokumentierter konsistenter SQLite-Backupbefehl und Restore-Smoke-Test.
- Keine Adminroute öffentlich exponieren.

**Prüfen**:

```bash
docker compose -f deploy/compose.yml config
docker build -t ongrow-support-control:test .
```

Erwartung: Exit 0, keine Secretwerte im gerenderten Beispiel.

### Schritt 7: Kontrollierter CI- und Integrationslauf

Nach ausdrücklicher Freigabe Repositories/Branches committen und pushen. Einen
lokalen oder isolierten CI-Integrationstest ausführen:

1. Test-hbbs stellt Attestierung aus.
2. Testclient enrollt.
3. API enthält genau ein Gerät.
4. Re-Enrollment erzeugt keinen zweiten Datensatz.
5. manipulierter Request wird abgelehnt.
6. Heartbeat aktualisiert nur `last_seen_at`.

Noch nicht produktiv deployen.

## Testplan

- Attestierungs-Interoperabilität Go/Rust.
- Tabellengetriebene Tests für jedes manipulierte Feld.
- Replay derselben Nonce.
- ID-Konflikt mit anderem Public Key.
- idempotentes Enrollment desselben Keys.
- zulässige Geräte-ID-Änderung nach neuer Serverattestierung.
- Request-/Bodylimits und unbekannte Felder.
- Rate Limit hinter nicht vertrauenswürdigem und vertrauenswürdigem Proxy.
- DB Busy/Fehler liefert sicheren 503.
- Client Backoff, Resume, Offline und parallele Timer.
- Container-Health und SQLite-Restore im isolierten Test.

## Fertigkriterien

- [ ] Eigenständiges lokales Control-Plane-Repo mit grüner Go-/Docker-CI.
- [ ] Kein Shared-DB-Zugriff auf `hbbs`.
- [ ] Enrollment akzeptiert nur gültige Serverattestierung plus
      Gerätesignatur.
- [ ] Gerätekey-Fingerprint ist stabile Primäridentität; ID bleibt eindeutig.
- [ ] UUID wird nur gehasht verarbeitet.
- [ ] Replay, Größenlimits und Rate Limits sind getestet.
- [ ] Client enrollt erst online und nur über HTTPS.
- [ ] Heartbeats laufen gedrosselt und parallelitätssicher.
- [ ] Keine Admin-UI und keine Zugangsdaten wurden vorgezogen.
- [ ] Kein Secret oder konkrete Infrastrukturadresse ist versioniert.
- [ ] Planstatus ist aktualisiert.

## STOP-Bedingungen

Anhalten und berichten, wenn:

- Plan 002 nicht vollständig umgesetzt und getestet ist;
- die API eine Clientbehauptung ohne Serverattestierung akzeptieren müsste;
- ein globaler Enrollment-Token vorgeschlagen wird;
- rohe UUID, private Geräteschlüssel oder permanente Signaturen gespeichert
  werden sollen;
- die RustDesk-DB geteilt oder direkt gemountet werden müsste;
- HTTP statt HTTPS für nichtlokale Clients erforderlich scheint;
- ein Admin-Endpunkt ohne Authentifizierung mitgebaut werden soll;
- unbekannte fremde Änderungen in einem betroffenen Arbeitsbaum liegen;
- Repo-Erstellung, Push oder Deployment nicht freigegeben ist.

## Wartungshinweise

- „Online“ bedeutet später `last_seen_at` innerhalb eines definierten
  Heartbeatfensters, nicht LAN-Erkennung.
- Der API-Vertrag muss versioniert bleiben. Neue Metadaten nur optional
  ergänzen.
- SQLite reicht für den Pilot. Erst bei gemessener Schreibkonkurrenz oder
  Hochverfügbarkeit migrieren.
- Das Control Plane enthält noch keine Secrets; Logs und Backups trotzdem als
  personenbezogene Betriebsdaten behandeln.
