# Plan 005: Deaktivierte Vault-, RBAC- und Auditgrundlage bereitstellen

> **Executor-Anweisung**: Diesen Plan Schritt für Schritt ausführen. Nach
> jedem Schritt die angegebene Prüfung ausführen und erst bei erfolgreichem
> Ergebnis fortfahren. Bei einer STOP-Bedingung anhalten und berichten, nicht
> improvisieren. Ausschließlich nicht produktive Testschlüssel verwenden.
> Dieser Plan erzeugt, überträgt oder aktiviert keine produktiven Schlüssel
> und verändert kein Kundengerät. Push, Pull Request und Deployment benötigen
> jeweils eine neue ausdrückliche Betreiberfreigabe.
>
> **Drift-Prüfung (zuerst ausführen)**:
>
> ```bash
> git -C /Users/schrobo/Developer/ongrow/ongrow-support-control diff --stat \
>   90923190b778f8d75fbb80086f4e7026abd72ae9..HEAD -- \
>   cmd internal migrations openapi docs deploy Dockerfile go.mod go.sum web
> git -C /Users/schrobo/Developer/ongrow/server-manager-authentik diff --stat \
>   7be4acb377ff16ec4d8d4d500055f9733b07e22c..HEAD -- \
>   platform/backup
> ```
>
> Jede Ausgabe bedeutet Drift. Dann die Abschnitte „Aktueller Zustand“ und
> „Umfang“ mit dem Live-Code abgleichen. Widersprechen sich Plan und Code,
> STOP und Bericht statt Umsetzung.

## Status

- **Status**: IN PROGRESS (lokale Prüfungen grün; Docker-Daemon/CI für Container-Gate fehlt)
- **Priorität**: P1
- **Aufwand**: XL
- **Risiko**: HIGH
- **Abhängig von**: `plans/003-signiertes-device-enrollment.md`,
  `plans/004-admin-inventar-und-zuordnung.md`
- **Kategorie**: security
- **Geplant bei**:
  - Support Control `90923190b778f8d75fbb80086f4e7026abd72ae9`
  - Server-Manager `7be4acb377ff16ec4d8d4d500055f9733b07e22c`
  - Client nur als Referenz `408784b9883af1a30e2ad215a57d8c484d806eba`
  - 2026-08-10

## Warum das wichtig ist

Plan 004 liefert ein OIDC-geschütztes Inventar, speichert aber bewusst keine
Zugangsdaten. Bevor ein Kunde unbeaufsichtigten Zugriff freigeben kann, muss
das Control Plane ein vom Gerät verschlüsseltes Secret sicher annehmen,
verschlüsselt speichern, rotieren, widerrufen, auditieren und nach einem
Backup nachweisbar wiederherstellen können. Dieser Plan baut nur diese
standardmäßig deaktivierte Serverseite; die ausdrückliche Kundenzustimmung,
lokale RustDesk-Konfiguration und tatsächliche Aktivierung folgen in Plan 006.

## Aktueller Zustand

### Support Control

- Repository: `/Users/schrobo/Developer/ongrow/ongrow-support-control`
- `internal/config/config.go` lädt die hbbs-Datei und das OIDC-Clientsecret
  aus Dateien. Es gibt noch keine Vault-Konfiguration.
- `internal/application/canonical.go` erzeugt für Enrollment und Heartbeat
  längenpräfixierte, versionsgebundene Signaturdaten. Neue Geräteoperationen
  müssen exakt dieses Muster mit eigenem Kontext verwenden.
- `internal/persistence/sqlite.go` führt eingebettete SQL-Migrationen aus und
  speichert Replay-Nonces in `device_nonces`. Diese Tabelle ist für die neuen
  signierten Geräteoperationen wiederzuverwenden.
- `internal/persistence/admin.go` schreibt Adminmutation und
  `admin_audit_events` in einer Transaktion. Secretmutation und
  `security_audit_events` müssen dieselbe Atomizität erhalten.
- `internal/api/server.go` begrenzt JSON-Bodies auf 16 KiB, lehnt unbekannte
  Felder ab, setzt Sicherheitsheader und `Cache-Control: no-store`. Neue
  Endpunkte dürfen diese Schutzschicht nicht umgehen.
- `internal/authn/oidc.go` und `internal/api/admin.go` erzwingen die OIDC-
  Rollen `admin` und `support`. Es wird keine neue Passwortauthentifizierung
  und keine neue globale Rolle eingeführt.
- Es existieren nur die Migrationen `001_initial.sql`,
  `002_admin_inventory.sql` und `003_oidc_sessions.sql`.
- `README.md` verspricht derzeit ausdrücklich, keine Passwörter oder
  Fernzugriffs-Secrets zu speichern. Diese Aussage erst ändern, wenn sie die
  neue verschlüsselte und standardmäßig deaktivierte Funktion präzise erklärt.

### Infrastruktur und Backup

- Repository:
  `/Users/schrobo/Developer/ongrow/server-manager-authentik`
- `platform/backup/support-control-backup.sh` sichert SQLite-Datenbank,
  Compose-Datei, hbbs-Public-Key und OIDC-Clientsecret verschlüsselt in das
  vorhandene Restic-Repository.
- `platform/backup/support-control-restore-drill.sh` stellt in einen isolierten
  Arbeitsbereich wieder her, erwartet aktuell exakt die Migrationen 001–003
  und startet das echte Image mit `--network none`.
- `platform/backup/tests/support-control-backup-entrypoints-test.sh` prüft die
  Schutz- und Reportmarker der beiden Entry-Points.
- Bestehende produktive Backups dürfen auch ohne Vault-Dateien weiterlaufen.

### Clientfähigkeit, aber nicht Clientumfang

- `src/ui_interface.rs` im Client stellt bereits
  `set_permanent_password_with_result` bereit; der vorhandene IPC-Pfad kann
  ein permanentes Passwort lokal setzen.
- Der Client verwendet bereits `sodiumoxide = "0.2"`; dessen
  `crypto::sealedbox` ist mit libsodium Sealed Boxes kompatibel. Das
  `sodiumoxide`-Projekt bezeichnet sich inzwischen als deprecated. Plan 005
  fügt deshalb keine weitere Rust-Abhängigkeit hinzu und ändert keinen
  Clientcode. Plan 006 muss die bestehende Nutzung isolieren; Plan 008 muss
  deren langfristigen Ersatz bewerten.

## Verbindliche Sicherheits- und Kryptoverträge

### Bedrohungsgrenze

- Die Verschlüsselung im Ruhezustand schützt gegen eine allein entwendete
  Datenbankdatei.
- Sie schützt nicht gegen einen kompromittierten laufenden Control-Plane-
  Prozess, Root-Zugriff auf den Host oder den gleichzeitigen Diebstahl von
  Datenbank und Keyrings.
- Datenbank und Keyrings landen zur Wiederherstellbarkeit im selben bereits
  verschlüsselten Restic-Repository. Das ist eine bewusste operative
  Recovery-Abwägung und muss im Bedrohungsmodell ausdrücklich stehen.
- Klartext darf nur kurzzeitig im Go-Prozess zwischen Ingest-Entschlüsselung
  und At-rest-Verschlüsselung existieren; niemals in SQLite, Browserantwort,
  Log, Trace, Fehlermeldung, Core-Dump oder Test-Fixture.

### Feature-Gate

- Neue Konfiguration: `CONTROL_UNATTENDED_VAULT_ENABLED`, Standard `false`.
- Bei `false` sind keine Vault-Keyrings nötig. Die neuen Geräte-Endpunkte
  verhalten sich wie nicht registrierte Routen und liefern JSON-404. Bestehende
  Funktionen, Backups und Restore-Drills bleiben unverändert funktionsfähig.
- Bei `true` müssen beide Keyring-Dateien vorhanden und gültig sein; sonst
  bricht der Prozess vor dem Listen ab und Readiness wird nie erreicht:
  - `CONTROL_VAULT_MASTER_KEYRING_FILE`
  - `CONTROL_VAULT_INGEST_KEYRING_FILE`
- Keyring-Dateien müssen regulär, nicht gruppen-/weltbeschreibbar und nur
  über read-only Mounts eingebunden sein. Fehler nennen nur Dateityp und
  Fehlerklasse, nie Schlüsselmaterial.

### Ingest-Verschlüsselung

- Verfahren: libsodium-kompatible Sealed Box auf Curve25519.
- Go-Implementierung: `golang.org/x/crypto/nacl/box.OpenAnonymous`; die
  Abhängigkeit exakt versioniert in `go.mod`/`go.sum` aufnehmen.
- Referenzen:
  - <https://doc.libsodium.org/public-key_cryptography/sealed_boxes>
  - <https://pkg.go.dev/golang.org/x/crypto/nacl/box>
- Aktiver Public Key und dessen `key_id` werden nur einem bereits enrollten
  Gerät nach gültiger Ed25519-Signatur geliefert. Alte Ingest-Private-Keys
  bleiben während einer dokumentierten Grace-Periode ausschließlich zum
  Entschlüsseln erhalten.

### Verschlüsselung im Ruhezustand

- Verfahren: AES-256-GCM aus der Go-Standardbibliothek mit zufälliger Nonce
  aus `crypto/rand`.
- Authenticated Additional Data ist eine kanonische, längenpräfixierte
  Bytefolge aus:
  - Kontext `ongrow-control-unattended-at-rest-v1`
  - unveränderlichem Gerätefingerprint
  - 16-Byte-Grant-Generation
  - Master-`key_id`
- Die Datenbank speichert Ciphertext, Nonce, Algorithmus-/Formatversion,
  Generation und Key-IDs getrennt. Niemals ein Passwort oder eine reversible
  Zwischenstufe in einer Textspalte speichern.
- Entschlüsselter Inhalt bleibt `[]byte`, wird nicht in `string` konvertiert
  und nach Gebrauch bestmöglich mit `clear()` überschrieben. Die
  Dokumentation darf daraus keine Garantie gegen Speicherforensik ableiten.

### Binäres Secret-Envelope

Version 1 enthält ausschließlich:

1. ein Byte Formatversion `0x01`;
2. eine zufällige 16-Byte-Grant-Generation;
3. eine unsigned 16-Bit-Längenangabe in Big Endian;
4. exakt so viele Passwortbytes.

Erlaubte Passwortlänge: 16 bis 128 Bytes. Nachlaufende Bytes, unbekannte
Versionen, falsche Generation oder ungültige Länge werden abgelehnt. Das
Control Plane interpretiert die Passwortbytes nicht als Text.

### Dauerhafter Serverzustand

- Persistiert werden nur `active` und `revoked`.
- `preparing`, `revoking`, `incomplete` und `error` sind transiente
  Clientzustände aus Plan 006 und gehören nicht in die Serverdatenbank.
- Je Gerätefingerprint existiert höchstens ein aktueller Grant. Ersetzen
  erzeugt eine neue Generation und ein neues Audit-Event; alte Ciphertexte
  werden nicht als History dupliziert.

## Benötigte Befehle

Alle Support-Control-Befehle im Repository
`/Users/schrobo/Developer/ongrow/ongrow-support-control` ausführen:

| Zweck | Befehl | Erwartung |
|---|---|---|
| Go-Tests | `mise exec -- go test ./...` | Exit 0 |
| Race-Tests | `mise exec -- go test -race ./...` | Exit 0 |
| Vet | `mise exec -- go vet ./...` | Exit 0 |
| Go-Build | `mise exec -- go build ./cmd/control-api` | Exit 0 |
| Format | `test -z "$(gofmt -l .)"` | Exit 0, keine Ausgabe |
| Web-Typecheck | `pnpm --dir web typecheck` | Exit 0 |
| Web-Tests | `pnpm --dir web test` | alle grün |
| Web-Lint | `pnpm --dir web lint` | Exit 0 |
| Web-Build | `pnpm --dir web build` | Exit 0 |
| Container | `docker build --tag ongrow-support-control:test .` | Exit 0 |

Server-Manager-Befehle im Repository
`/Users/schrobo/Developer/ongrow/server-manager-authentik`:

| Zweck | Befehl | Erwartung |
|---|---|---|
| Shell-Syntax | `bash -n platform/backup/support-control-backup.sh platform/backup/support-control-restore-drill.sh platform/backup/tests/support-control-backup-entrypoints-test.sh` | Exit 0 |
| Entry-Point-Test | `bash platform/backup/tests/support-control-backup-entrypoints-test.sh` | Meldung `passed` |

## Umfang

**Im Umfang – Support Control**:

- `docs/security/unattended-access-threat-model.md` (neu)
- `docs/adr/0003-unattended-vault-cryptography.md` (neu)
- `internal/config/config.go`, `internal/config/config_test.go`
- `internal/vault/` (neu: Keyrings, Envelope, AES-GCM, Sealed-Box-Ingest,
  Rotation und Tests)
- `internal/domain/` (Grant- und Security-Audit-Typen)
- `internal/application/canonical.go` und zugehörige Tests
- `internal/application/service.go` und zugehörige Tests
- `internal/persistence/sqlite.go`, `internal/persistence/admin.go` nur soweit
  die sichere Statusprojektion im Inventar es verlangt
- `internal/persistence/unattended.go` und Tests (neu)
- `internal/api/server.go`, `internal/api/admin.go` und zugehörige Tests
- `migrations/004_unattended_vault.sql`, `migrations/migrations.go`
- `cmd/control-api/main.go` sowie bei Bedarf neue Dateien im selben Paket für
  `vault verify` und `vault rewrap`
- `openapi/`, `README.md`, `deploy/compose.yml`, `Dockerfile`
- `go.mod`, `go.sum`
- `web/src/` nur für eine secretsichere Statusanzeige, falls das vorhandene
  API-Feld sonst nicht sinnvoll sichtbar ist

**Im Umfang – Server-Manager**:

- `platform/backup/support-control-backup.sh`
- `platform/backup/support-control-restore-drill.sh`
- `platform/backup/tests/support-control-backup-entrypoints-test.sh`
- `platform/backup/backup.env.example`
- `platform/backup/README.md`

**Außerhalb des Umfangs**:

- jede Änderung in `rustdesk-lab` außerhalb `plans/`
- permanentes Passwort lokal setzen oder RustDesk-Optionen verändern
- Kundenzustimmung, Aktivierungs-/Widerrufs-UI oder automatischer Upload
- Techniker-Schlüsselpaar, Secret-Ausgabe an einen Techniker oder Verbindung
- Browser-Endpunkt für Ciphertext oder Klartext
- neue Authentifizierung, neue globale Rolle oder Ersatz von Authentik
- produktives Schlüsselmaterial, produktive Keyring-Dateien oder deren Inhalt
- Änderungen am laufenden VPS, Traefik, DNS, systemd-Timern oder Restic-Repo
- Push, Pull Request und Deployment ohne jeweils ausdrückliche Freigabe

## Git-Arbeitsweise

- Support Control: vom geprüften Stand einen Branch
  `feature/003-unattended-vault` erstellen.
- Server-Manager: separater Branch
  `feat/support-control-vault-backup-20260810`.
- Conventional Commits passend zur Historie, zum Beispiel:
  - `feat: add disabled unattended access vault`
  - `feat: verify unattended vault backups`
- Logische Commits je Repository; keine Test-Keyrings, produktiven Secrets,
  Restore-Arbeitsbereiche oder Binärdateien committen.
- Vor jedem Commit `git status --short` prüfen. Fremde Änderungen nicht
  übernehmen.
- Nicht pushen, keinen PR öffnen und nicht deployen, solange der Betreiber
  dies nicht separat freigegeben hat.

## Schritte

### Schritt 1: Bedrohungsmodell und ADR festschreiben

In `docs/security/unattended-access-threat-model.md` Akteure,
Vertrauensgrenzen, Datenfluss, erlaubte Klartextphase, Replay, Log-/Core-Dump-
Risiko, Datenbankdiebstahl, Hostkompromittierung, Schlüsselverlust und die
Restic-Recovery-Abwägung dokumentieren.

In `docs/adr/0003-unattended-vault-cryptography.md` die oben verbindlich
festgelegten Formate, Algorithmen, Key-IDs, Rotation, Feature-Gate und die
Abgrenzung zu Plan 006/007 festhalten. Dazu begründen, warum keine eigene
Kryptoprimitive und keine Secret-Ausgabe an den Browser zulässig ist.

**Prüfen**:

```bash
test -s docs/security/unattended-access-threat-model.md
test -s docs/adr/0003-unattended-vault-cryptography.md
! rg -n '(password|private_key|master_key)[[:space:]]*[:=][[:space:]]*[A-Za-z0-9+/]{16,}' docs
```

Erwartung: Exit 0; keine eingebetteten Schlüssel oder Passwörter.

### Schritt 2: Feature-Gate, Keyrings und Kryptoschicht implementieren

1. `Config` in `internal/config/config.go` um eine optionale
   `UnattendedVaultConfig` ergänzen. Bei deaktiviertem Gate keine Dateien
   öffnen. Bei aktiviertem Gate beide Pfade gemeinsam verlangen.
2. Unter `internal/vault/` zwei getrennte, versionierte JSON-Keyringformate
   implementieren:
   - Master-Keyring: Formatversion, aktive Key-ID, Liste von 32-Byte-Keys und
     Status `active` oder `decrypt-only`.
   - Ingest-Keyring: Formatversion, aktive Key-ID, Curve25519 Public/Private-
     Keypair und Status `active` oder `decrypt-only`.
3. Strikt parsen: unbekannte JSON-Felder, doppelte Key-IDs, mehrere aktive
   Schlüssel, ungültiges Base64, falsche Längen und widersprüchliche
   Public/Private-Keypairs ablehnen.
4. Envelope-Parser, `SealAtRest`, `OpenAtRest` und `OpenIngest` als kleine
   Interfaces implementieren. Produktionscode akzeptiert keine Nonce-Quelle
   außer `crypto/rand`; Tests dürfen sie injizieren.
5. Klartextpuffer mit `defer clear(buffer)` bestmöglich löschen. Logger und
   Fehler dürfen weder Eingabe noch Ciphertext formatieren.
6. `golang.org/x/crypto/nacl/box` exakt pinnen. Einen festen, ausschließlich
   nicht produktiven Interoperabilitätsvektor als Binär-Fixture oder Go-
   Literal testen, der mit libsodium/sodiumoxide erzeugt wurde. In der Fixture
   darf nur ein erkennbares Testpasswort vorkommen.

**Prüfen**:

```bash
mise exec -- go test ./internal/config ./internal/vault
mise exec -- go test -race ./internal/vault
test -z "$(gofmt -l internal/config internal/vault)"
```

Erwartung: alle Tests grün; deaktivierter Start braucht keine Vault-Dateien,
aktivierter Start lehnt jede unvollständige/ungültige Keyringkombination ab.

### Schritt 3: Migration, Domain und atomare Persistenz ergänzen

`migrations/004_unattended_vault.sql` legt an:

- `unattended_access_grants`
  - Primär-/Fremdschlüssel: `device_key_fingerprint`
  - `generation` als 16-Byte-BLOB
  - `state` mit `CHECK (state IN ('active','revoked'))`
  - Ingest-Key-ID und -Formatversion
  - Master-Key-ID, At-rest-Algorithmus und -Formatversion
  - At-rest-Nonce und -Ciphertext als BLOB
  - Consent-Version und Consent-Zeitpunkt
  - created/updated/revoked timestamps
  - keine Passwort-, Klartext- oder frei befüllbare Secretspalte
- `security_audit_events`
  - unveränderliche Event-ID und Zeit
  - `actor_kind`, `actor_subject`, `action`, `target_fingerprint`, `result`,
    `request_id`
  - eng begrenzte sichere Metadaten ohne Secret/Ciphertext

Unter `internal/domain/` nur `active`/`revoked` modellieren. Unter
`internal/persistence/unattended.go` folgende Transaktionen implementieren:

- neuen/ersetzten Grant plus erfolgreiches Audit atomar schreiben;
- Widerruf plus Audit atomar schreiben und Ciphertext/Nonce im selben Commit
  entfernen oder kryptografisch unbrauchbar machen;
- Replay-Nonce über die bestehende `device_nonces`-Tabelle einfügen;
- Grant für Rotation streamend/batchweise lesen und per Generation als
  Compare-and-swap aktualisieren;
- keine Audit-History mit alten Ciphertexten anlegen.

`security_audit_events` wird nur über Methoden geschrieben, die keine
beliebigen Metadaten-Maps aus API-Eingaben übernehmen.

**Prüfen**:

```bash
mise exec -- go test ./internal/persistence ./internal/domain
mise exec -- go test -race ./internal/persistence
mise exec -- go test ./...
```

Erwartung: Migration von einer 003-Testdatenbank gelingt; Rollbacktests zeigen,
dass Grant, Nonce und Audit nie nur teilweise geschrieben werden.

### Schritt 4: Signierte Geräte-Endpunkte hinter dem Gate ergänzen

Nur bei aktiviertem Vault registrieren:

- `POST /v1/device/unattended-access/key`
  - authentifiziertes enrolltes Gerät;
  - liefert nur aktive Ingest-`key_id`, Algorithmus und Public Key.
- `PUT /v1/device/unattended-access`
  - akzeptiert Key-ID, Formatversion, Generation, Consent-Version/-Zeit,
    Request-ID, versiegeltes Envelope, Timestamp, Nonce und Ed25519-Signatur;
  - entschlüsselt Ingest, validiert Envelope/Generation, verschlüsselt sofort
    mit aktivem Master-Key und persistiert atomar mit Audit.
- `POST /v1/device/unattended-access/revoke`
  - akzeptiert Generation, Request-ID, Timestamp, Nonce und Signatur;
  - ist für dieselbe bereits widerrufene Generation idempotent, lehnt eine
    fremde/veraltete aktive Generation aber als Konflikt ab;
  - gibt nie Secretmaterial zurück.

In `internal/application/canonical.go` pro Operation einen eigenen
versionsgebundenen Kontext definieren. Die Signatur muss alle semantischen
Felder abdecken; für das versiegelte Envelope entweder die vollständigen
Bytes oder einen SHA-256-Digest der Bytes kanonisch aufnehmen. Niemals nur
Geräte-ID und Timestamp signieren. Fingerprint aus dem gespeicherten
Device-Key ableiten, nicht aus Requestdaten übernehmen.

Bestehende Grenzen beibehalten: höchstens 16 KiB Request, unbekannte Felder
ablehnen, zulässiges Zeitfenster und einmalige Nonce wie bei Heartbeat,
strukturierte Fehlercodes, `Cache-Control: no-store`. Bei deaktiviertem Gate
müssen alle drei Pfade JSON-404 liefern.

**Prüfen**:

```bash
mise exec -- go test ./internal/application ./internal/api ./internal/persistence
mise exec -- go test -race ./internal/application ./internal/api ./internal/persistence
```

Erwartung: Happy Path grün; Manipulation jedes signierten Feldes,
Replay-Nonce, abgelaufener Timestamp, falsche Key-ID, manipulierte Sealed Box,
zu großes Envelope und nicht enrolltes Gerät werden abgelehnt. Keine Antwort
enthält Ingest-Ciphertext, At-rest-Ciphertext oder Klartext.

### Schritt 5: Sichere Statusprojektion und bestehende RBAC erweitern

Die Admin-Geräteliste und Detailantwort dürfen ausschließlich liefern:

- `unattended_access_status`: `not_configured`, `active` oder `revoked`
- Consent-Version und Consent-Zeitpunkt, sofern vorhanden
- Zeitpunkt der letzten Statusänderung

Keine Key-ID, Generation, Nonce, Ciphertext oder kryptografische
Fehlerdetails an Browserendpunkte geben. `support` darf diese Metadaten lesen;
für Plan 005 gibt es keine Browsermutation. Die bestehende OIDC-RBAC bleibt
maßgeblich. Falls `web/src/` angepasst wird, nur einen textlich verständlichen
Status anzeigen; keine Aktivierungs-, Copy- oder Connect-Schaltfläche.

OpenAPI und `README.md` aktualisieren. In der README deutlich zwischen
„standardmäßig deaktivierte verschlüsselte Vault-Grundlage“ und „noch nicht
implementierter Kundenzustimmung/Verbindung“ unterscheiden.

**Prüfen**:

```bash
mise exec -- go test ./internal/api ./internal/persistence
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web lint
pnpm --dir web build
rg -n 'unattended_access_status' openapi README.md internal web/src
```

Erwartung: alle Befehle erfolgreich; Treffer zeigen ausschließlich Status und
Metadaten, keine Secretfelder.

### Schritt 6: Verify und resumierbare Rotation bereitstellen

`cmd/control-api` um offline nutzbare Subcommands ergänzen:

- `control-api vault verify`
  - öffnet Datenbank und beide Keyrings read-only;
  - prüft Referenzen, Key-IDs und Entschlüsselbarkeit aller aktiven Grants;
  - gibt nur Zähler und `ok`/Fehlerklasse aus, keine Gerätefingerprints,
    Key-IDs, Nonces, Ciphertexte oder Klartexte.
- `control-api vault rewrap --target-key-id <id>`
  - entschlüsselt jeweils einen Datensatz und verschlüsselt mit dem Zielkey;
  - aktualisiert per Generation/aktuellem Key als Compare-and-swap;
  - ist unterbrechbar und bei Wiederholung idempotent;
  - löscht keinen alten Schlüssel.

Ein alter Master-Key darf erst manuell aus dem Keyring entfernt werden, wenn
`vault verify` null Referenzen darauf meldet. Ingest-Rotation aktiviert einen
neuen Public Key und behält alte Private Keys für eine dokumentierte
Grace-Periode als `decrypt-only`.

**Prüfen**:

```bash
mise exec -- go test ./cmd/control-api ./internal/vault ./internal/persistence
mise exec -- go test -race ./...
mise exec -- go build ./cmd/control-api
```

Erwartung: Unterbrechungs-/Wiederholungstest der Rotation bleibt konsistent;
`vault verify` erkennt manipulierte und unbekannt verschlüsselte Datensätze,
ohne Material auszugeben.

### Schritt 7: Backup und isolierten Restore optional Vault-fähig machen

Im Server-Manager `SUPPORT_CONTROL_VAULT_ENABLED` mit Standard `false`
einführen. Bei `false` müssen die bestehenden Backups unverändert gelingen.
Bei `true` sind beide Dateien zwingend:

- `SUPPORT_CONTROL_MASTER_KEYRING_FILE`, Standardpfad unter
  `/srv/secrets/ongrow-support-control/`
- `SUPPORT_CONTROL_INGEST_KEYRING_FILE`, gleicher Secret-Bereich

Die Backup-Logik verlangt entweder beide oder keine Datei, sichert beide im
verschlüsselten Restic-Snapshot und schreibt weder Pfadinhalte noch Checksums
der Keyrings in öffentliche Logs. `backup.env.example` enthält nur leere
Beispielpfade/Flags.

Der Restore-Drill muss alte Snapshots ohne Vault-Keyrings und neue Snapshots
mit vollständigem Paar unterscheiden. Ein einzelner Keyring ist immer ein
Fehler. Für neue Snapshots:

1. Migration 004 und Tabellen prüfen.
2. beide Keyrings nur read-only in den weiterhin mit `--network none`
   gestarteten Testcontainer mounten;
3. Vault-Gate nur im isolierten Container aktivieren;
4. `control-api vault verify` ausführen;
5. ausschließlich folgende neue Reportmarker ergänzen:
   `vault_keys_present=ok` und `vault_ciphertexts=ok`.

Tests verwenden temporäre, eindeutig nicht produktive Keyrings und einen
Fake-Restic-/Fake-Docker-Pfad nach vorhandenem Testmuster. Keine laufenden
Timer oder Produktionsdateien anfassen.

**Prüfen**:

```bash
bash -n platform/backup/support-control-backup.sh \
  platform/backup/support-control-restore-drill.sh \
  platform/backup/tests/support-control-backup-entrypoints-test.sh
bash platform/backup/tests/support-control-backup-entrypoints-test.sh
```

Erwartung: Syntaxprüfung Exit 0, Test meldet `passed`; Fälle „Gate aus“,
„beide Keyrings“, „nur einer vorhanden“ und „manipulierter Ciphertext“ sind
automatisiert abgedeckt.

### Schritt 8: Gesamtabnahme ohne Deployment durchführen

Im Support-Control-Repo:

```bash
mise exec -- go test ./...
mise exec -- go test -race ./...
mise exec -- go vet ./...
mise exec -- go build ./cmd/control-api
test -z "$(gofmt -l .)"
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web lint
pnpm --dir web build
docker build --tag ongrow-support-control:test .
git status --short
```

Im Server-Manager-Repo:

```bash
bash -n platform/backup/support-control-backup.sh \
  platform/backup/support-control-restore-drill.sh \
  platform/backup/tests/support-control-backup-entrypoints-test.sh
bash platform/backup/tests/support-control-backup-entrypoints-test.sh
git status --short
```

Zusätzlich mit ausschließlich temporären Test-Keyrings einen lokalen
Container-Smoke-Test für Gate `false` und Gate `true` ausführen. Der
deaktivierte Container muss ohne Keyrings starten; der aktivierte Container
mit vollständigen Test-Keyrings; fehlender oder halber Keyring muss vor dem
Listen abbrechen. Kein VPS und kein externes Restic-Repository verwenden.

Erwartung: alle Prüfungen grün; `git status --short` enthält nur die in diesem
Plan genannten Dateien. Danach Status noch nicht auf DONE setzen, solange der
isolierte Backup-/Restore-Test mit Migration 004 und `vault verify` nicht
nachweislich erfolgreich war.

## Testplan

- `internal/config/config_test.go`: Gate-Standard, gültiges Paar, fehlende
  Hälfte, unsichere Dateirechte, falsche Längen und unbekannte Felder.
- `internal/vault/*_test.go`: Envelope-Grenzen, Sealed-Box-Interop,
  AES-GCM-Roundtrip, falsche AAD/Generation/Key-ID, manipulierte Ciphertexte,
  Rotation, leere Puffer nach Gebrauch soweit testbar.
- `internal/application/service_test.go`: Signaturen über jedes Feld,
  Zeitfenster, Replay, Enrollmentbindung, Idempotenz und Konflikte.
- `internal/persistence/*_test.go`: Migration 003→004, atomare Grant-/Nonce-/
  Audittransaktionen, Widerruf ohne Secretreste, Compare-and-swap-Rotation.
- `internal/api/server_test.go` und `internal/api/admin_test.go`: 404 bei Gate
  aus, Bodylimit, unbekannte Felder, sichere Fehler, keinerlei Secretfelder in
  Geräte- oder Browserantworten, Rollen `admin`/`support`.
- `cmd/control-api/*_test.go`: `verify` und unterbrechbare `rewrap`-Läufe mit
  ausschließlich sicheren Zählerausgaben.
- `platform/backup/tests/support-control-backup-entrypoints-test.sh`: alte und
  neue Snapshots, vollständiges Keyringpaar, Teilpaar-Fehler, Migration 004,
  Network-Isolation und neue Reportmarker.
- Strukturelles Vorbild: vorhandene Tests in `internal/application`,
  `internal/persistence`, `internal/api` sowie der bestehende
  Support-Control-Backup-Entry-Point-Test.

## Fertigkriterien

- [ ] Feature-Gate ist standardmäßig `false`; bestehender Betrieb benötigt
  keine Vault-Dateien.
- [ ] Aktivierter Betrieb verlangt zwei gültige read-only Keyrings und bricht
  bei jeder Teil-/Fehlkonfiguration sicher ab.
- [ ] Datenbank enthält nur AES-256-GCM-Ciphertext und sichere Metadaten,
  niemals ein permanentes Passwort im Klartext.
- [ ] Signierte Geräteoperationen sind an Fingerprint, Payload, Generation,
  Timestamp und Einmal-Nonce gebunden und gegen Replay getestet.
- [ ] Dauerhafte Zustände sind ausschließlich `active` und `revoked`.
- [ ] Browser und Admin-API erhalten weder Klartext noch Ciphertext, Nonce,
  Generation oder Key-ID.
- [ ] Grantmutation, Replay-Nonce und Security-Audit sind atomar.
- [ ] `vault verify` und resumierbares `vault rewrap` funktionieren mit
  Testdaten und geben keine sensiblen Identifikatoren aus.
- [ ] Backup bleibt bei ausgeschaltetem Vault kompatibel; bei eingeschaltetem
  Vault werden beide Keyrings verschlüsselt gesichert und isoliert geprüft.
- [ ] Alle Go-, Web-, Shell- und Containerprüfungen sind grün.
- [ ] Kein Kundengerät, kein VPS, kein DNS, kein Timer und kein produktives
  Restic-Repository wurde verändert.
- [ ] Keine Test- oder Produktionsschlüssel sind versioniert.
- [ ] Nach nachgewiesener Abnahme ist die Statuszeile in `plans/README.md`
  aktualisiert.

## STOP-Bedingungen

Anhalten und berichten, wenn:

- Plan 003 oder 004 nicht DONE ist oder die Drift-Prüfung einen Widerspruch
  zu diesem Plan zeigt;
- eine Lösung Klartext, Ciphertext oder Schlüsselmaterial an Browser, Logs,
  Traces, Fehlermeldungen oder SQLite geben würde;
- ein Feature-Gate-freier Zwang zur Vault-Konfiguration entstünde;
- eine eigene Kryptoprimitive nötig erscheint oder Go/libsodium-Sealed-Box-
  Interoperabilität nicht durch Testvektor nachgewiesen werden kann;
- die Implementierung Passwörter in Go-`string` konvertieren müsste;
- Migration oder Widerruf vorhandene Geräte-/Auditdaten unkontrolliert löscht;
- Rotation alte Keys automatisch entfernt oder nicht idempotent ist;
- Backup nur einen der beiden Keyrings enthält oder der Restore-Drill Netzwerk
  benötigt;
- produktives Schlüsselmaterial, ein Deployment, Push, PR, VPS-/Traefik-/
  Timer- oder Restic-Zugriff erforderlich wird, ohne dass dies separat
  ausdrücklich freigegeben wurde;
- eine Verifikation nach einem vernünftigen Korrekturversuch weiterhin
  fehlschlägt oder Dateien außerhalb des Umfangs geändert werden müssten.

## Wartungshinweise

- Plan 006 implementiert erst die ausdrückliche, widerrufbare Zustimmung,
  Passworterzeugung, lokale RustDesk-Konfiguration und verschlüsselte
  Übergabe. Bis dahin bleibt das Gate produktiv aus.
- Plan 007 darf Grants niemals über den Browser ausgeben. Er benötigt ein
  registriertes Techniker-Schlüsselpaar und kurzlebige, gerätegebundene
  Verbindungsfreigaben.
- Vor Entfernen eines Master-Keys immer `vault verify` ausführen und null
  Referenzen nachweisen. Alte Ingest-Private-Keys erst nach Ablauf der
  dokumentierten Grace-Periode entfernen.
- Das bereits genutzte Rust-`sodiumoxide` ist deprecated:
  <https://github.com/sodiumoxide/sodiumoxide>. Plan 008 muss einen gepflegten
  Ersatz oder eine klar begründete, isolierte Übergangslösung festlegen.
- Backup von Datenbank und Keyrings im selben verschlüsselten Restic-
  Repository priorisiert Wiederherstellbarkeit. Eine spätere Trennung in
  unabhängige Trust Domains ist eine bewusste Sicherheitsentscheidung, kein
  stiller Refactor.
- Reviewer müssen besonders auf Secretkopien, `string`-Konvertierungen,
  Fehlerformatierung, Core-Dumps, atomare Transaktionen, AAD-Bindung,
  Rotation und Restore-Nachweis achten.
