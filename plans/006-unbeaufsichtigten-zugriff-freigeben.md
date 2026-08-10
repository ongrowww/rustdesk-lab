# Plan 006: Unbeaufsichtigten Zugriff ausdrücklich freigeben und widerrufen

Status: TODO
Priorität: Hoch
Abhängigkeiten: Plan 002, Plan 003 und Plan 005
Geplant am: 2026-07-30

## Ziel

Kunden können OnGROW den unbeaufsichtigten Zugriff mit einer klaren,
freiwilligen Einwilligung aktivieren und genauso verständlich wieder
widerrufen. Der Client setzt die erforderlichen RustDesk-Optionen und ein
starkes permanentes Passwort erst nach Bestätigung und überträgt dieses
ausschließlich verschlüsselt an das Control Plane.

Die Funktion ist standardmäßig deaktiviert. Installation, Update oder bloßes
Öffnen der App gelten niemals als Zustimmung.

## Ausgangslage

- Der Client kann ein permanentes Passwort über den vorhandenen IPC-Pfad mit
  Rückmeldung setzen.
- Relevante RustDesk-Optionen sind unter anderem
  `verification-method`, `approve-mode`,
  `allow-only-conn-window-open` und
  `allow-logon-screen-password`.
- Plan 005 stellt einen verschlüsselten Geräte-Endpunkt, Zustandsmodell,
  Schlüsselverwaltung, RBAC und Audit bereit.
- Die bestehende OnGROW-Oberfläche hat bereits Hilfen für
  Betriebssystemberechtigungen. Deren tatsächlicher Status darf nicht mit der
  gesonderten Zustimmung zum unbeaufsichtigten Zugriff vermischt werden.

## Produktentscheidung

Die Startseite erhält einen eigenen Bereich „Unbeaufsichtigter Zugriff“ mit
Status und erklärendem Dialog. Der primäre Schalter ist kein stiller Toggle:
Vor dem Aktivieren zeigt ein Modal Zweck, Reichweite, Widerruf und
Sicherheitsfolgen in verständlicher Sprache. Die finale Aktion lautet
beispielsweise „Zugriff für OnGROW freigeben“.

Vor der UI-Implementierung wird der Zustand in der bestehenden Figma-Datei
für deaktiviert, wird vorbereitet, aktiv, unvollständig, Fehler und Widerruf
entworfen. Offizielle Icons und vorhandene OnGROW-Tokens werden wiederverwendet.

## Umsetzungsschritte

### 1. Native Fähigkeiten sauber kapseln

- Im Rust-Core eine dedizierte Schnittstelle für Aktivierung, Statusprüfung
  und Widerruf des OnGROW-verwalteten Zugriffs anlegen.
- Vor jeder Änderung prüfen:
  - läuft der installierte RustDesk-Service,
  - kann der IPC-Pfad schreiben,
  - sind erforderliche Systemberechtigungen vorhanden,
  - existiert bereits ein permanentes Passwort.
- Ein vorhandenes, nicht nachweislich von OnGROW verwaltetes Passwort niemals
  überschreiben oder löschen. Stattdessen einen Konfliktstatus mit
  verständlicher Handlungsanweisung zurückgeben.
- Die vorherigen nicht geheimen Werte der betroffenen Optionen lokal
  versioniert sichern, damit ein Widerruf sie wiederherstellen kann.

### 2. Secret lokal erzeugen und anwenden

- Das Passwort ausschließlich mit dem kryptografisch sicheren
  Zufallszahlengenerator des Betriebssystems erzeugen.
- Mindestens 24 zufällige Zeichen beziehungsweise eine gleichwertige
  Entropie verwenden; keine menschenlesbaren Muster oder Geräteinformationen
  einbauen.
- Das Secret niemals über Flutter-Logs, Crash-Reports, Zwischenablage,
  Analytics oder persistente UI-Zustände führen.
- Nach expliziter Bestätigung folgende Zielkonfiguration über den bestehenden
  IPC-Pfad setzen:
  - `verification-method=use-permanent-password`
  - `approve-mode=password`
  - `allow-logon-screen-password=Y`
  - `allow-only-conn-window-open=N`
- Jede native Änderung durch die vorhandene IPC-Bestätigung verifizieren.

### 3. Verschlüsselte Übergabe transaktional gestalten

- Vor der Aktivierung den aktuellen öffentlichen Ingest-Schlüssel samt Key-ID
  vom Control Plane beziehen und dessen Vertrauenskette prüfen.
- Das neue Passwort noch im nativen Core für diesen Schlüssel versiegeln.
- Die verschlüsselte Payload mit Gerätesignatur, Einmal-Nonce,
  Zustimmungszeitpunkt und Version des Zustimmungstexts senden.
- Den lokalen Zustand erst nach erfolgreicher Serverbestätigung als
  `enabled` anzeigen.
- Falls das Setzen lokal gelingt, die Serverübernahme aber endgültig
  scheitert:
  - das gerade erzeugte Passwort wieder entfernen,
  - vorherige Optionswerte wiederherstellen,
  - den Zustand als Fehler melden.
- Ein vorbestehendes Passwort darf bei keinem Rollback verändert werden.

### 4. Sicheren Widerruf implementieren

- Widerruf ebenfalls über einen bestätigenden Dialog auslösen.
- Zuerst den lokalen Zugriff deaktivieren und die zuvor gesicherten Optionen
  wiederherstellen.
- Erst danach den serverseitigen Grant widerrufen und Secret-Material
  kryptografisch unbrauchbar beziehungsweise gelöscht markieren.
- Ist der Server nicht erreichbar, gilt lokale Sicherheit als führend:
  Zugriff bleibt lokal deaktiviert, der serverseitige Widerruf wird mit
  begrenztem Backoff wiederholt und als `revoking` angezeigt.
- Einen Widerruf niemals davon abhängig machen, dass ein Administrator online
  ist.

### 5. Flutter-Oberfläche und FFI anbinden

- Zustandsbehaftete FFI-Methoden statt eines globalen Freitext-Status
  bereitstellen.
- Den Figma-Entwurf in der OnGROW-Support-Oberfläche umsetzen.
- Status klar unterscheiden:
  - Nicht freigegeben
  - Wird vorbereitet
  - Freigegeben
  - Aktion erforderlich
  - Wird widerrufen
  - Fehler
- Bei fehlenden macOS-Berechtigungen direkt in die bestehende
  plattformspezifische Hilfe verlinken.
- Tastaturbedienung, Fokusfalle, Escape-Verhalten, Screenreader-Beschriftung
  und ausreichend große Klickflächen prüfen.

### 6. Control-Plane-Status ergänzen

- In Geräte-API und Admin-Inventar ausschließlich Status,
  Zustimmungszeitpunkt, Textversion und relevante
  Berechtigungs-/Fehlerhinweise ausgeben.
- In der Admin-Liste klar zwischen „Gerät online“ und
  „Unbeaufsichtigter Zugriff freigegeben“ unterscheiden.
- Kein Button zum Anzeigen, Kopieren oder Exportieren des Passworts
  anbieten.

## Fehler- und Wiederanlaufmatrix

Automatisierte Tests müssen mindestens diese Fälle abdecken:

- bestehendes fremdverwaltetes permanentes Passwort,
- fehlender Dienst oder fehlende IPC-Rechte,
- fehlende macOS-Systemberechtigung,
- veralteter oder unbekannter Ingest-Key,
- lokales Setzen schlägt vor der Übertragung fehl,
- Übertragung schlägt nach lokalem Setzen fehl,
- Prozessabbruch in jeder Zustandsphase,
- doppeltes Aktivieren und doppeltes Widerrufen,
- Widerruf bei offline Control Plane,
- App-Update und Neustart bei aktivem Grant.

## Tests und Verifikation

- Rust-Unit-Tests für Zustandsmaschine, Generierung, Rollback und
  Wiederanlauf.
- FFI-Tests für strukturierte Resultate und Fehlercodes.
- Flutter-Widget-Tests für alle Zustände und Modalabläufe.
- Plattformtest auf einem nicht produktiven macOS-Gerät mit installiertem
  Dienst.
- Sicherstellen, dass Logs, Crash-Ausgaben und persistente
  Flutter-Einstellungen kein Secret enthalten.
- End-to-End-Test mit Test-Control-Plane und ausschließlich Testschlüsseln.
- Bestehende Client-CI einschließlich macOS-Build und Smoke-Test ausführen.

## Dokumentation

- Zustimmungstext, Zweck, Reichweite und Widerrufsweg in deutscher Sprache
  dokumentieren und versionieren.
- Support-Runbook für Konflikt mit bestehendem Passwort, unvollständigen
  Zustand und Offline-Widerruf erstellen.
- Klarstellen, dass die Freigabe keinen automatischen Zugriff durch beliebige
  RustDesk-Nutzer erlaubt.

## Abnahmekriterien

- Ohne explizite Bestätigung bleibt unbeaufsichtigter Zugriff deaktiviert.
- Ein fremdes bestehendes Passwort wird niemals überschrieben.
- Das generierte Passwort verlässt den nativen Prozess nur verschlüsselt.
- Ein fehlgeschlagener Aktivierungsablauf hinterlässt keinen unbemerkten
  lokalen Zugang.
- Widerruf deaktiviert den lokalen Zugriff auch bei Serverausfall.
- UI und Admin-Inventar zeigen den tatsächlichen, nach Neustart
  rekonstruierten Zustand.
- Es existiert weiterhin keine Browserfunktion zur Passwortanzeige.

## STOP

- Keine Aktivierung auf Kunden- oder Produktivgeräten.
- Kein reales Secret in Tests, Logs, Screenshots, Issues oder CI-Artefakten
  verwenden.
- Kein Deployment ohne erneute, ausdrückliche Freigabe.
- Plan 007 erst beginnen, wenn Aktivierung, Rollback und Widerruf im
  End-to-End-Test zuverlässig bestehen.
