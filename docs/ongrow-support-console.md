# OnGROW Support Console für macOS

Die **OnGROW Support Console** ist der getrennte, ausgehende Techniker-Client.
Sie kann parallel zum eingehenden **OnGROW Support Desk** installiert werden.

## Produktgrenzen

- App-Name: `OnGROW Support Console`
- Bundle-ID: `de.ongrow.supportconsole`
- URL-Schema: `ongrow-support-console://`
- Netzwerkrolle: ausschließlich ausgehend; Rendezvous-Registrierung und lokales
  TCP-Listening werden im Produktprofil abgeschaltet.
- Private Ed25519- und X25519-Schlüssel liegen im macOS-Schlüsselbund.
- Weder Browser noch Flutter erhalten das permanente Gerätepasswort.

## Registrierung

1. Konsole starten. Beim ersten Start erzeugt der native Kern ein Ed25519- und
   ein X25519-Schlüsselpaar.
2. In der Konsole beide öffentlichen Schlüssel kopieren und Support Control
   öffnen.
3. Unter „Konsolen“ einen Anzeigenamen und beide öffentlichen Schlüssel
   eintragen.
4. Support Control öffnet
   `ongrow-support-console://registered/<console-id>`.
5. Die native Konsole signiert einen Claim. Erst nach erfolgreicher Prüfung der
   Signatur und beider Fingerprints wird die Konsolen-ID im Schlüsselbund
   gespeichert.

## Verbindung starten

1. In Support Control ein Gerät mit aktiver, vom Kunden erteilter Freigabe
   auswählen.
2. „Supportverbindung starten“ bestätigen. Die OIDC-Anmeldung darf höchstens
   fünf Minuten alt sein.
3. Der Browser erhält ein einmalig nutzbares, 60 Sekunden gültiges Ticket und
   öffnet ausschließlich
   `ongrow-support-console://launch/<ticket>`.
4. Die native Konsole löst das Ticket mit einem signierten
   Proof-of-Possession ein. Support Control versiegelt das Passwort neu für den
   registrierten X25519-Schlüssel der Konsole.
5. Der Rust-Kern entschlüsselt das Passwort, verbraucht es einmalig beim
   nativen Sessionstart und bestätigt diesen Start signiert. Ticket, Envelope
   und Passwort werden nicht geloggt.

## Lab-Build

Der manuell startbare Workflow
`.github/workflows/ongrow-support-console-macos-arm64.yml` erzeugt ein
ad-hoc-signiertes ARM64-DMG. Vor dem Build müssen dieselben drei GitHub-
Repository-Variablen wie für den Support Desk vorhanden sein:

- `ONGROW_RENDEZVOUS_HOST`
- `ONGROW_HBBS_PUBLIC_KEY`
- `ONGROW_CONTROL_PLANE_URL`

Der Workflow prüft Produktname, Bundle-ID, URL-Schema, Architektur,
eingebettetes Serververtrauen, Rust-Tests, Codesignatur und einen Start-Smoke-
Test. Ein produktiver Rollout erfordert weiterhin Apple Developer ID und
Notarisierung.
