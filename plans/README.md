# Implementierungspläne

Erstellt und zuletzt abgeglichen mit dem Improve-Skill am 10.08.2026. Die Pläne
sind in der angegebenen Reihenfolge auszuführen, sofern ihre Abhängigkeiten
nichts anderes vorgeben. Vor der Ausführung ist der jeweilige Plan vollständig
zu lesen. STOP-Bedingungen und Freigabepunkte sind verbindlich. Nach Abschluss
aktualisiert der ausführende Agent die betreffende Statuszeile.

## Reihenfolge und Status

| Plan | Titel | Priorität | Aufwand | Abhängig von | Status |
|------|-------|-----------|---------|--------------|--------|
| 001 | Reproduzierbaren unsigned macOS-ARM64-Baseline-Build in GitHub Actions einrichten | P1 | M | — | DONE |
| 002 | Serverbestätigte Geräteidentität einführen | P1 | L | 001 | DONE |
| 003 | Signiertes Device-Enrollment und Heartbeat aufbauen | P1 | L | 002 | DONE (`5dfcd4a`) |
| 004 | Authentifiziertes Admin-Inventar und Kundenzuordnung umsetzen | P1 | L | 003 | DONE (`9092319`) |
| 005 | Vault-, RBAC- und Auditgrundlage bereitstellen | P1 | XL | 003, 004 | IN PROGRESS (CI inkl. Container grün; isolierter Restic-Restore-Drill ausstehend) |
| 006 | Unbeaufsichtigten Zugriff ausdrücklich freigeben und widerrufen | P1 | XL | 002, 003, 005 | TODO |
| 007 | Autorisierte Technikerverbindung ohne Passwortanzeige umsetzen | P1 | XL | 004, 005, 006 | TODO |
| 008 | Produktionsfreigabe, Wiederherstellung und Compliance absichern | P1 | XL | 002–007 | TODO |

Statuswerte: TODO | IN PROGRESS | DONE | BLOCKED (mit kurzer Begründung) |
REJECTED (mit kurzer Begründung)

## Abhängigkeiten

- Plan 001 hat keine Vorgänger. Er schafft die verifizierte Build-Baseline,
  auf der Branding, eigene Serverkonfiguration, Signierung und weitere
  Plattformen später getrennt aufbauen können.
- Plan 002 bindet die vom hbbs-Server vergebene RustDesk-ID kryptografisch an
  das konkrete Gerät. Er muss Client und Server gemeinsam und
  rückwärtskompatibel erweitern.
- Plan 003 verwendet diese Serverbestätigung für ein neues, separates
  Go-Control-Plane. Erst damit entsteht ein vertrauenswürdiges
  Geräteinventar; hbbs allein wird nicht zur Admin-Datenbank umfunktioniert.
- Plan 004 setzt auf dem Enrollment auf und ergänzt OIDC-geschützte
  Kunden-/Gerätezuordnung und eine Admin-Oberfläche. Er speichert noch keine
  Zugangsdaten.
- Plan 005 muss vor jeder Passwortautomatisierung abgeschlossen werden. Er
  schafft Schlüsselverwaltung, verschlüsselte Ablage, RBAC, Audit, Rotation
  und Restore.
- Plan 006 führt erst danach die ausdrückliche Zustimmung, lokale
  Passworterzeugung, verschlüsselte Übergabe und den sicheren Widerruf im
  Kundenclient ein.
- Plan 007 stellt das Secret ausschließlich einem registrierten,
  autorisierten Technikerclient kurzlebig und gerätegebunden bereit. Der
  Browser sieht kein Passwort.
- Plan 008 ist das verbindliche Produktions-Gate. Ein technischer Pilot aus
  Plan 002 bis 007 ist noch kein freigegebenes Kundenprodukt.

## Zielarchitektur

- `rustdesk-lab`: dünner OnGROW-Client-Fork für Kundenoberfläche,
  Geräteidentität, Zustimmung und lokale RustDesk-Konfiguration.
- `rustdesk-server-lab`: dünner hbbs/hbbr-Fork für eigene IDs und signierte
  Gerätebestätigungen; keine allgemeine Admin- oder Secret-API.
- `ongrow-support-control`: separates Go-Control-Plane für Enrollment,
  Heartbeats, OIDC/RBAC, Kunden-/Gerätezuordnung, Audit und verschlüsselte
  Grants.
- Admin-Weboberfläche: React und TypeScript, vom Control Plane ausgeliefert
  und ausschließlich über OIDC-Sitzungen erreichbar.
- Technikerzugriff: registrierter lokaler Client mit gerätegebundenem
  Schlüsselpaar; niemals Passwortanzeige oder Passwortkopie im Browser.

Die unveränderliche Geräteidentität ist ein kryptografischer Fingerprint.
Die sichtbare RustDesk-ID und der Anzeigename bleiben änderbare Attribute und
dürfen nicht als Primärschlüssel für Berechtigungen dienen.

## Ausführungsnotizen

- Plan 001: Lokale Implementierung und statische Review am 25.07.2026
  erfolgreich. Isolierter Commit `9e0434e8`; Push, Pull Request, Merge,
  Actions-Aktivierung und Remote-Build warten auf Betreiberfreigabe. Branch
  und PR #1 wurden anschließend freigegeben und erstellt. Dabei starteten die
  geerbten PR-Trigger `CI` und `Full Flutter CI` unerwartet; beide Runs wurden
  sofort abgebrochen. Die beiden daraufhin bei GitHub registrierten
  Upstream-Workflows wurden auf `disabled_manually` gesetzt. Die übrigen
  Upstream-Entry-Points sind weiterhin nicht registriert. PR #1 wurde am
  26.07.2026 per Rebase in `master` gemergt (`09401be4`), ohne einen weiteren
  Run auszulösen. Der erste manuelle Baseline-Run
  `30212061232` endete am 26.07.2026 erfolgreich. ARM64-App-Prüfung,
  DMG-Verifikation, Artefakt-Upload und unabhängiger SHA-256-Abgleich waren
  erfolgreich. Das finale Artefakt ist etwa 25 MB groß und wird sieben Tage
  aufbewahrt.
- Plan 002 bis 008 wurden am 30.07.2026 als aufeinander aufbauende
  Ausführungspläne erstellt. Dabei wurden Client- und Serverpfade gezielt
  rekonstruiert; es fand kein vollständiges Audit des gesamten
  RustDesk-Upstreams statt.
- Plan 002: Der zusätzliche öffentliche Thin Fork `ongrowww/hbb_common`
  versioniert den identischen Protobuf-Vertrag auf getrennten Client- und
  Serverbaselines. Server-Commit `5e0d0c0` bestand Tests, Release-Build,
  Containerbau und Smoke-Test in Run `30585768383`. Client-Commit `e70da87c`
  bestand Flutter-Test und -Analyse, Bridge-Generierung,
  Attestierungs-Unit-Tests, macOS-ARM64-Build, Signaturprüfung und
  Start-Smoke-Test in Run `30585782818`. Es erfolgte noch kein
  Serverdeployment.
- Plan 003: Das separate Control Plane wurde mit Commit `5dfcd4a` für das
  Deployment vorbereitet. Signiertes Enrollment, Heartbeats, Containerbetrieb
  und der macOS-Pilot wurden erfolgreich verifiziert. Dieser Commit ist der
  Ausgangspunkt für die Drift-Prüfung von Plan 004.
- Plan 004: Der Pilot wurde bis Commit `9092319` abgeschlossen. Authentik ist
  als selbst betriebener OIDC-Provider mit MFA und Rollenclaims eingerichtet.
  Login, Rollen, CSRF-Schutz und Logout wurden Ende-zu-Ende im Browser
  abgenommen; die temporäre Basic Auth wurde danach entfernt. GitHub-Actions-
  Lauf `30954578047` war erfolgreich. Authentik und Support Control werden
  täglich verschlüsselt per Restic gesichert; isolierte Restore-Drills waren
  erfolgreich.
- Plan 005 wurde am 10.08.2026 gegen Support Control `9092319`,
  Server-Manager `7be4acb` und den Client `408784b9` neu geplant. Er bleibt
  eine standardmäßig deaktivierte Serverseiten-Grundlage und verändert noch
  kein Kundengerät.
- Plan 005: Die lokale Implementierung wurde als Support-Control-Commit
  `8eccecb` und Server-Manager-Commit `cab5384` gepusht. GitHub-Actions-Lauf
  `31437160660` war einschließlich Web-, Format-, Go-, Race-, Vet-, Binary-
  und Containerprüfung erfolgreich. Vor `DONE` fehlt weiterhin der
  vollständige isolierte Restic-Restore-Drill mit Migration 004, beiden
  nicht produktiven Test-Keyrings und `vault verify`.

## Bewusst verworfene oder vertagte Ansätze

- Den vorhandenen wiederverwendbaren Workflow
  `.github/workflows/flutter-build.yml` direkt aufzurufen, wurde verworfen:
  Er baut nicht nur macOS ARM64, sondern zahlreiche Plattformen und enthält
  Release-, Signier- und Veröffentlichungslogik. Das ist für den ersten,
  kontrollierten Versuch zu breit.
- Ein Build vom jeweils aktuellen `master` wurde für die Baseline verworfen.
  Plan 001 baut reproduzierbar den vollständigen Commit des stabilen Tags
  `1.4.9`; ein späterer Plan kann die kontrollierte Aktualisierung übernehmen.
- Blacksmith und Self-hosted Runner bleiben Optimierungen nach einer stabilen,
  messbaren GitHub-Actions-Baseline. Sie verändern die Sicherheitsarchitektur
  der Pläne 002 bis 008 nicht.
- Die hbbs-Datenbank direkt als Kunden- und Admin-Inventar zu verwenden, wurde
  verworfen. Ihre aktuelle Peer-Information reicht dafür nicht aus und würde
  Rendezvous-Betrieb, Produktdaten und Secret-Verwaltung unnötig koppeln.
- Ein festes Standardpasswort, ein aus Geräteinformationen abgeleitetes
  Passwort oder ein Passwort in Client-Konfiguration, CI oder
  Umgebungsvariablen wurde aus Sicherheitsgründen verworfen.
- Eine Passwortanzeige, ein Copy-Button oder ein Passwort im
  `mailto:`-/Custom-URL-Workflow wurde verworfen. Der Browser erhält nur eine
  kurzlebige, undurchsichtige Verbindungsanforderung.
- Eine automatische Freigabe bei Installation oder Update wurde verworfen.
  Unbeaufsichtigter Zugriff benötigt eine ausdrückliche, nachvollziehbare und
  widerrufbare Kundenzustimmung.
- Eine Desktop-Vorschau, Screenshots oder Sitzungsaufzeichnung im
  Admin-Inventar sind vertagt und würden eine eigene Datenschutz- und
  Sicherheitsbewertung benötigen.
- Windows wird nach dem macOS-Pilot als eigener Plattform-Track behandelt.
  Linux folgt wegen seiner Desktop- und Display-Server-Varianten separat.
- Code Signing, macOS-Notarisierung, Update-Vertrauen, Datenschutz und
  AGPL-Veröffentlichung sind nicht optional; sie sind bewusst im
  Produktions-Gate Plan 008 gebündelt.
