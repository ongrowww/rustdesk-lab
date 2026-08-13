# OnGROW: unbeaufsichtigten Zugriff betreiben

## Umfang und Status

Diese Funktion gehört zu Plan 006 und ist bis zum erfolgreichen CI- und
Ende-zu-Ende-Test ausschließlich ein Entwicklungsstand. Sie darf nicht auf
Kunden- oder Produktivgeräten aktiviert werden.

Die Funktion ist standardmäßig nicht freigegeben. Installation, Update,
Enrollment und das bloße Öffnen von OnGROW Support Desk gelten nicht als
Zustimmung.

## Zustimmungstext

Aktuelle Textversion: `de-v1-2026-08-12`

Vor der Aktivierung bestätigt die Person am Gerät ausdrücklich:

> OnGROW darf dieses Gerät bei Supportbedarf öffnen, auch wenn gerade niemand
> davor sitzt. Du kannst die Freigabe jederzeit widerrufen.

Die Oberfläche erklärt zusätzlich, dass das Passwort geheim bleibt und
verschlüsselt übertragen wird. Die Zustimmung wird mit Zeitpunkt und
Textversion an das Control Plane übermittelt.

## Sicherheitsablauf

1. Der Client prüft OnGROW-Geräte-ID, Verbindung, installierten Dienst,
   Schreibbarkeit des IPC-Pfads und die erforderlichen macOS-Berechtigungen.
2. Ein bereits vorhandenes, nicht nachweislich von OnGROW gesetztes
   permanentes Passwort wird weder überschrieben noch gelöscht.
3. Der Client sichert die bisherigen nicht geheimen RustDesk-Optionen und
   erzeugt lokal ein kryptografisch zufälliges Passwort.
4. Er setzt und verifiziert über den Dienst:
   - `verification-method=use-permanent-password`
   - `approve-mode=password`
   - `allow-logon-screen-password=Y`
   - `allow-only-conn-window-open=N`
5. Das Secret wird als libsodium-kompatible Sealed Box für den aktuellen
   öffentlichen Ingest-Key verschlüsselt. Nur Ciphertext, Gerätesignatur,
   Nonce und Zustimmungsmetadaten verlassen den Clientpfad.
6. Erst die erfolgreiche Serverbestätigung setzt den lokalen Zustand auf
   „Freigegeben“. Bei einem Fehler werden Passwort und Optionen
   zurückgerollt.

Der lokale Zustand enthält kein Klartextpasswort. Ein Fingerprint des
RustDesk-Passwortspeichers dient ausschließlich dazu, beim Widerruf zu
beweisen, dass weiterhin das von OnGROW gesetzte Passwort vorliegt. Nach einer
manuellen Passwortänderung verweigert der Client das Löschen.

## Widerruf

Der Widerruf sperrt den lokalen Zugriff zuerst und stellt anschließend die
zuvor gesicherten Optionen wieder her. Danach widerruft der Client den Grant
im Control Plane.

Ist das Control Plane nicht erreichbar, bleibt der lokale Zugriff gesperrt.
Der Client zeigt „Wird widerrufen“ und wiederholt ausschließlich die
serverseitige Bestätigung mit begrenztem exponentiellem Backoff. Ein
Administrator muss dafür nicht online sein.

## Supportfälle

### „Anderes permanentes Passwort vorhanden“

- Das Passwort nicht automatisiert ersetzen oder löschen.
- RustDesk-Einstellungen gemeinsam mit der verantwortlichen Person prüfen.
- Erst nach deren bewusster Entscheidung den Konflikt beseitigen und die
  Freigabe erneut starten.

### „Passwort wurde nach der Freigabe geändert“

- OnGROW Support Desk verändert das neue Passwort nicht.
- In den RustDesk-Einstellungen klären, wem die neue Konfiguration gehört.
- Den Zustand nicht durch manuelles Bearbeiten der lokalen OnGROW-Metadaten
  umgehen.

### „Aktion erforderlich“ wegen macOS-Berechtigungen

- Die integrierte Einrichtungshilfe öffnen.
- Bildschirmaufnahme, Bedienungshilfen und Eingabeüberwachung einzeln prüfen.
- Danach den Status in OnGROW Support Desk aktualisieren und die Freigabe
  erneut starten.

### „Wird widerrufen“ bleibt sichtbar

- Der lokale Zugriff ist bereits gesperrt.
- Erreichbarkeit des Control Planes prüfen; kein Passwort neu setzen.
- Nach Wiederherstellung der Verbindung den Client geöffnet lassen, damit der
  begrenzte Wiederholungsablauf die Serverbestätigung abschließt.

## Diagnose ohne Geheimnisse

Erlaubt sind Statuscode, Phase, Zeitpunkt, Textversion und Gerätefingerprint.
Nicht in Logs, Tickets, Screenshots oder Zwischenablage gehören:

- Klartextpasswort;
- RustDesk-Passwortspeicher oder Salt;
- Sealed Envelope, Generation, Nonce oder Signatur;
- Ingest- oder Master-Keymaterial.

Das Admin-Inventar zeigt ausschließlich `not_configured`, `active` oder
`revoked` sowie sichere Zustimmungsmetadaten. Es gibt keine Browserfunktion
zum Anzeigen, Kopieren oder Exportieren des Passworts.

## Noch erforderliche Freigaben

Vor einem Pilotbetrieb müssen Client-CI, macOS-Build, Bridge-Generierung,
Test-Control-Plane und ein isolierter Ende-zu-Ende-Test erfolgreich sein. Das
Server-Feature-Gate und produktive Keyrings dürfen erst in einem getrennt
freigegebenen Deployment aktiviert werden.
