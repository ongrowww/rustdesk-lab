# Plan 004: Authentifiziertes Admin-Inventar mit Kunden- und Gerätezuordnung bauen

> **Executor-Anweisung**: Diesen Plan erst ausführen, wenn das signierte
> Enrollment aus Plan 003 produktionsnah testbar ist. Keine eigene
> Passwortauthentifizierung erfinden. Bei fehlendem OIDC-Anbieter anhalten und
> die Betreiberentscheidung einholen.
>
> **Drift-Prüfung (zuerst ausführen)**:
>
> ```bash
> git -C /Users/schrobo/Developer/ongrow/ongrow-support-control diff --stat \
>   <PLAN-003-DONE-SHA>..HEAD -- \
>   internal migrations openapi web deploy
> ```
>
> `<PLAN-003-DONE-SHA>` vor Ausführung aus `plans/README.md` beziehungsweise
> dem abgeschlossenen Plan-003-Commit einsetzen. Fehlt dieser SHA, anhalten.

## Status

- **Status**: DONE (Support Control `9092319`, 2026-08-10)
- **Priorität**: P1
- **Aufwand**: L
- **Risiko**: MED
- **Abhängig von**: `plans/003-signiertes-device-enrollment.md`
- **Kategorie**: direction
- **Geplant bei**: Client `29723655`, Server `df5b912`, 2026-07-30

## Abschlussnachweis

- Authentik ist als selbst betriebener OIDC-Provider eingerichtet; MFA,
  Rollenclaims sowie die Rollen `admin` und `support` wurden praktisch
  geprüft.
- OIDC-Login, serverseitige Session, CSRF-Schutz und Logout wurden im Browser
  Ende-zu-Ende abgenommen. Die temporäre Basic Auth wurde anschließend
  entfernt.
- Der finale Stand des Control Planes ist Commit
  `90923190b778f8d75fbb80086f4e7026abd72ae9`. Der zugehörige GitHub-Actions-
  Lauf `30954578047` war erfolgreich.
- Verschlüsselte Restic-Backups für Authentik und Support Control wurden
  erstellt und ausschließlich isoliert wiederhergestellt und geprüft. Die
  täglichen systemd-Timer sind aktiv.
- Die Fertigkriterien dieses Plans sind damit erfüllt. Unbeaufsichtigter
  Zugriff und die Ablage von Zugangsdaten bleiben ausdrücklich Gegenstand der
  Pläne 005 und 006.

## Warum das wichtig ist

Signiertes Enrollment schafft verlässliche Gerätedaten, aber noch keine für
Supportmitarbeiter nutzbare Übersicht. Dieser Plan ergänzt eine
OIDC-geschützte Adminoberfläche, sprechende Gerätenamen und die Zuordnung zu
Kundenorganisationen. Danach kann OnGROW alle enrollten Supportgeräte,
Onlinezustand, Plattform, App-Version und Berechtigungsstand sehen — jedoch
noch keine unbeaufsichtigten Zugangsdaten abrufen.

## Aktueller Zustand und Begriffe

- Plan 003 liefert `devices`, signierte Heartbeats und eine interne
  Geräteidentität auf Basis des Ed25519-Key-Fingerprints.
- „Geräte im Netzwerk“ bedeutet in diesem Produkt ausschließlich:
  **erfolgreich im OnGROW Control Plane enrollte Clients**. Es findet kein
  LAN-Scan statt.
- Die bestehende Kunden-App verwendet folgende Design-Tokens
  (`flutter/packages/ongrow_support_ui/lib/ongrow_support_view.dart:6-11`):
  - Violet `#7516F8`
  - Violet Dark `#381061`
  - Ink `#1C1425`
  - Muted `#61596B`
  - Canvas `#FCFCFE`
  - Surface `#F9F8FB`
- Icons sollen aus einer offiziellen, konsistenten Iconbibliothek kommen,
  keine handgezeichneten Platzhalter.
- Das neue Control-Plane-Repo bleibt die Quelle für API und Admin-Frontend:

  ```text
  ongrow-support-control/
    web/                 # React + TypeScript + Vite
    internal/admin/      # OIDC, Sessions, RBAC, Admin-API
    internal/webassets/  # eingebetteter Produktionsbuild
  ```

## Authentifizierungsentscheidung

- Produktionszugriff ausschließlich über einen vorhandenen oder bewusst
  bereitgestellten OIDC-Provider.
- Keine Basic Auth, keine hartcodierte Allowlist im Frontend, keine selbst
  entwickelte Passwortdatenbank.
- Authorization Code Flow mit PKCE.
- Sichere serverseitige Sessioncookie:
  `HttpOnly`, `Secure`, `SameSite=Lax`, begrenzte Laufzeit, Rotation nach Login.
- Rollen:
  - `admin`: Organisationen, Zuordnungen und Rollen verwalten.
  - `support`: Inventar lesen und Geräte öffnen.
- Serverseitige Autorisierung an jedem Adminendpunkt. Versteckte Buttons sind
  keine Zugriffskontrolle.
- OIDC-Clientsecret und Sessionkey ausschließlich aus Docker-Secrets/Files,
  nie aus Repo, Image, Browserbundle oder Logs.

## Datenmodell

Neue Migrationen:

- `organizations`
  - interne ID
  - eindeutiger Name
  - optionaler externer Referenzcode
  - Status aktiv/archiviert
- `device_assignments`
  - Gerätefingerprint eindeutig
  - Organisation optional
  - `display_name`
  - interne Notiz
  - Änderungszeit und ändernder Admin-Subject
- `admin_audit_events`
  - Event-ID, Zeit, OIDC-Subject, Aktion, Zieltyp/-ID
  - sichere strukturierte Metadaten ohne Tokens/Secrets

Das technische `device_name` des Clients bleibt unverändert erhalten.
`display_name` ist die bewusst gepflegte Supportbezeichnung.

## Benötigte Befehle

| Zweck | Befehl | Erwartung |
|---|---|---|
| Go-Tests | `go test -race ./...` | alle grün |
| Go-Vet | `go vet ./...` | Exit 0 |
| Go-Format | `gofmt -l .` | keine Ausgabe |
| Web-Abhängigkeiten | `pnpm --dir web install --frozen-lockfile` | Exit 0 |
| Web-Typecheck | `pnpm --dir web typecheck` | Exit 0 |
| Web-Tests | `pnpm --dir web test` | alle grün |
| Web-Lint | `pnpm --dir web lint` | Exit 0 |
| Web-Build | `pnpm --dir web build` | Exit 0 |
| Gesamtbuild | `docker build -t ongrow-support-control:test .` | Exit 0 |

## Vorgeschlagene Executor-Werkzeuge

- Vor UI-Implementierung Figma verwenden, falls verbunden. Den
  Geräteübersichts-Screen und Device-Detail-Screen als Designgate erstellen.
- Für UI-Review vorhandene Accessibility-, Layout-, Color-, Typography- und
  UI-Skills verwenden, falls verfügbar.

## Umfang

**Im Umfang**:

- OIDC-Login/Logout/Callback und serverseitige Sessions
- Rollen `admin` und `support`
- Organisationen und Gerätezuordnung
- Admin-JSON-Endpunkte
- React-/TypeScript-Adminoberfläche
- responsive Geräteliste und Detailansicht
- Audit für Login, Logout und mutierende Adminaktionen
- Tests und Produktionsbuild im bestehenden Control-Plane-Container

**Außerhalb des Umfangs**:

- permanente RustDesk-Passwörter
- Secret-Vault oder Secretanzeige
- Verbindung zu einem Gerät
- Aktivieren/Widerrufen unbeaufsichtigten Zugriffs
- Kundenlogin oder Kundenportal
- Screenshot-/Desktop-Vorschauen
- Abfragen der RustDesk-Serverdatenbank
- öffentliche Freigabe ohne OIDC

## Git-Arbeitsweise

- Branch: `feature/002-admin-inventory`
- Conventional Commits mit DCO:
  - `feat: add authenticated support inventory`
  - optional getrennt `feat: add device organization assignments`
- Lockfiles committen, keine generierten Secret-/OIDC-Konfigurationen.
- Push, Deployment und OIDC-Clientanlage benötigen separate Freigaben.

## Schritte

### Schritt 1: OIDC-Vertrag und Rollen als ADR dokumentieren

Im neuen Repo `docs/adr/0001-admin-authentication.md` festhalten:

- gewählter Issuer und Discoverymechanismus, ohne Secretwerte;
- Claims für stabile Subject-ID und Rollen;
- Sessiondauer und Logoutverhalten;
- Rollenmatrix;
- warum Basic Auth und lokale Passwörter ausgeschlossen sind.

Wenn kein OIDC-Provider verfügbar ist, hier STOP. Kein Ersatzmechanismus.

**Prüfen**:

```bash
test -s docs/adr/0001-admin-authentication.md
! rg -n '(client_secret|password|token)[[:space:]]*=' docs
```

Erwartung: Datei vorhanden, keine Credentialzuweisung.

### Schritt 2: Adminschema und serverseitige RBAC implementieren

Migrationen und Domain-/Repository-Layer ergänzen. Jede mutierende Aktion
atomar zusammen mit einem Audit-Event schreiben.

Adminendpunkte:

- `GET /v1/admin/devices`
- `GET /v1/admin/devices/{fingerprint}`
- `PATCH /v1/admin/devices/{fingerprint}/assignment`
- `GET|POST|PATCH /v1/admin/organizations`

Listenendpunkt mit serverseitiger Pagination, Suche und Filtern:

- Organisation
- online/offline
- OS
- Permission vollständig/unvollständig
- App-Version

Keine unlimitierte `SELECT *`-Antwort.

**Prüfen**: Go-Tests für jede Rolle und Object-ID → grün.

### Schritt 3: OIDC und sichere Sessions integrieren

- Discovery beim Start; Readiness bleibt false, wenn Authkonfiguration
  ungültig.
- State, Nonce und PKCE prüfen.
- Sessiondaten serverseitig oder authentifiziert verschlüsselt speichern.
- CSRF-Schutz für alle mutierenden Browserrequests.
- `Cache-Control: no-store` auf Admin- und Authantworten.
- Security Header einschließlich CSP, Frame-Ausschluss und
  `Referrer-Policy`.
- Keine Tokens in URLs nach Callback, Logs oder Browserstorage.

**Prüfen**: `httptest`-Suite für Login, Callback-Manipulation, Rollen,
CSRF und Logout → grün.

### Schritt 4: Figma-Designgate durchführen

Vor Produktiv-UI folgende Screens entwerfen und menschlich freigeben:

1. Login-/Fehlerzustand.
2. Geräteliste Desktop und schmal.
3. leere Liste.
4. Filter ohne Treffer.
5. Gerätedetail.
6. Zuordnungsdialog.
7. Offline-/veralteter-Client-Zustand.

Liste zeigt:

- Displayname, technische ID maskiert/auf Wunsch sichtbar
- Organisation
- Online/zuletzt gesehen
- OS und App-Version
- Berechtigungsstatus
- unbeaufsichtigter Zugriff zunächst nur als `Nicht verfügbar`

**Prüfen**: Freigegebener Figma-Link/Node im Planstatus dokumentiert. Ohne
Freigabe keine UI-Implementierung.

### Schritt 5: React-Adminoberfläche implementieren

Vite, React, TypeScript strict und eine offizielle Iconbibliothek verwenden.
Keine eigene Datenquelle im Browser; ausschließlich `/v1/admin`.

Barrierefreiheit:

- semantische Tabelle auf Desktop, sinnvolle Karten auf schmalen Viewports;
- vollständige Tastaturbedienung;
- sichtbare Focus-Ringe;
- Dialog-Focustrap und Focus-Rückgabe;
- Status nicht nur über Farbe;
- Live-Region für Speichern/Fehler;
- Reduced Motion beachten.

Fehlertexte dürfen keine internen Details enthalten.

**Prüfen**: Typecheck, Tests, Lint und Build → grün.

### Schritt 6: Produktionsassets einbetten und Same-Origin erzwingen

Frontendbuild in den Go-Binary-/Containerbuild integrieren. Im Produktivmodus:

- API und Web unter derselben Origin;
- keine breite CORS-Freigabe;
- unbekannte `/v1/`-Routen liefern JSON-404, kein SPA-Fallback;
- Assetdateien mit Hash langfristig cachen, HTML/Admin-JSON nicht.

**Prüfen**: Dockerbuild plus HTTP-Smoke-Test für Loginredirect, Assets,
unauthenticated 401/302 und CSP.

### Schritt 7: Kontrolliertes Pilotdeployment

Erst nach ausdrücklicher Freigabe:

- OIDC-Client anlegen;
- Secrets als Docker-Secrets bereitstellen;
- bestehende Traefik-Instanz nur um einen eng begrenzten Router/Service
  ergänzen;
- Datenvolume und Backuppfad bereitstellen;
- keine bestehenden Traefik- oder RustDesk-Dienste ersetzen.

Mit zwei Testrollen prüfen: `admin`, `support`. Keine echten Kundennamen für
den ersten Smoke-Test verwenden.

## Testplan

- OIDC-State/Nonce/PKCE, falscher Issuer/Audience, abgelaufene Tokens.
- Sessioncookie-Flags, Logout und Rotation.
- Serverseitige Rollenmatrix für jeden Endpunkt.
- IDOR: Supporter kann kein fremdes/ungültiges Objekt mutieren.
- CSRF auf allen Mutationen.
- Pagination, Suche, Filterkombinationen und leere Zustände.
- Onlineberechnung anhand Heartbeatgrenze.
- Organisation archivieren ohne Geräteverlust.
- Audit wird bei erfolgreicher Mutation geschrieben, nicht bei abgelehnter.
- UI Keyboard, Focus, Screenreaderlabels und Responsive Layout.
- Container-Smoke-Test hinter simuliertem Reverse Proxy.

## Fertigkriterien

- [x] OIDC-ADR und Rollenmatrix sind freigegeben.
- [x] Keine lokale Passwortauthentifizierung oder Basic Auth.
- [x] Alle Adminendpunkte erzwingen Auth und Rolle serverseitig.
- [x] Organisationen und Displaynamen sind getrennt von Clientmetadaten.
- [x] Geräteliste ist paginiert, filterbar und barrierefrei.
- [x] „Online“ basiert dokumentiert auf Heartbeats.
- [x] Mutationen erzeugen Audit-Events ohne Secrets.
- [x] Adminoberfläche zeigt keine Zugangsdaten und kann noch nicht verbinden.
- [x] Go-, Web- und Containerprüfungen sind grün.
- [x] Deployment änderte keine bestehenden Infrastrukturkomponenten.
- [x] Planstatus ist aktualisiert.

## STOP-Bedingungen

Anhalten und berichten, wenn:

- Plan 003 nicht DONE ist;
- kein geeigneter OIDC-Provider oder keine stabilen Rollenclaims verfügbar
  sind;
- Basic Auth, Browser-LocalStorage-Tokens oder ein eigenes Passwortsystem als
  Abkürzung nötig erscheinen;
- die UI Geräte direkt aus der RustDesk-SQLite lesen soll;
- konkrete Secrets in Compose, GitHub Variables oder Logs landen würden;
- das Figma-Designgate nicht freigegeben wird;
- Deployment bestehende Traefik-/RustDesk-Dienste überschreiben müsste;
- externe Änderungen nicht freigegeben sind.

## Wartungshinweise

- Admin-UI und API müssen als eine Sicherheitsgrenze reviewed werden.
- Organisationen sind zunächst interne Zuordnung, kein Kundenmandantenmodell.
- Vor Plan 005 keine Secretspalten oder Platzhalter-Passwörter hinzufügen.
- Der technische Gerätename kann sich ändern; Support-Displayname bleibt
  bewusst administrativ gepflegt.
