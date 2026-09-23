# Windows Support Console: Pilotabnahme

Die Windows-Konsole ist ein eigenes x64-Lab-Produkt. Das Artefakt muss
`OnGROW Support Console.exe`, `librustdesk.dll`, weitere Laufzeit-DLLs und
`data/` gemeinsam enthalten. Es ist unsigniert und nicht für den Rollout
freigegeben. Der Windows-Workflow, die VM-Prüfung und die Verbindungsprüfung
wurden lokal auf macOS nicht ausgeführt.

## Eigentum und Installation

`install_ongrow_windows_console.ps1` installiert nur unter dem aktuellen
Windows-Benutzer in `%LOCALAPPDATA%\Programs\OnGROW\Support Console`.
Das URI-Schema liegt unter
`HKCU\Software\Classes\ongrow-support-console`, der Uninstall-Eintrag unter
`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\OnGROWSupportConsole`.
Beide Einträge tragen `OnGROWOwner=de.ongrow.supportconsole`. Vorhandene
Einträge ohne diese Kennung werden nicht überschrieben. Die Deinstallation
vergleicht Kennung, Installationspfad und URI-Kommando vor dem Entfernen.
Desk und Original-RustDesk besitzen andere Namen und Registry-Pfade.

Der Installer darf nur mit einer freigegebenen Testidentität und einem
vollständigen Artefakt ausgeführt werden. Vor Upgrade und Deinstallation
die Konsole schließen. Keine echten Tickets, Passwörter oder Schlüssel in
Fehlerberichte oder Screenshots aufnehmen.

## Abnahmeprotokoll

Windows-Version: ausstehend. Artefakt-SHA: ausstehend. CI-Run-ID: ausstehend.
Test-Subject und Testgerät: nur lokal im freigegebenen Pilot dokumentieren.

| Fall | Erwartung | Ergebnis |
| --- | --- | --- |
| W01 | Desk, Original-RustDesk und Konsole bleiben getrennt startbar | BLOCKED: VM fehlt |
| W02 | Benutzer A registriert die Konsole; Benutzer B kann A-Schlüssel nicht lesen | BLOCKED: VM fehlt |
| W03 | Browser-URI registriert startet die kalte Konsole genau einmal | BLOCKED: VM fehlt |
| W04 | Browser-URI launch erreicht die bereits laufende Konsole genau einmal | BLOCKED: VM fehlt |
| W05 | Gültiges Ticket startet eine echte Verbindung zum Testgerät; ACK erst bei `connection_ready` | BLOCKED: VM fehlt |
| W06 | Replay, Ablauf und fremdes Ticket erzeugen keine Verbindung | BLOCKED: VM fehlt |
| W07 | Sperre, Widerruf und Rollenentzug verhindern neue Verbindungen | BLOCKED: VM fehlt |
| W08 | Benutzerneustart erhält die registrierte Identität | BLOCKED: VM fehlt |
| W09 | Upgrade erhält Identität und URI-Eigentum | BLOCKED: VM fehlt |
| W10 | Deinstallation entfernt nur Console-Einträge und Console-Dateien | BLOCKED: VM fehlt |
| W11 | Sentinel erscheint weder in Logs noch UI-Fehlern oder Prozessargumenten | BLOCKED: VM fehlt |
| W12 | Windows-Startup-Test zeigt initialisierte Console-UI | PENDING |

Für jeden Fall nach Durchführung Windows-Version, finalen Git-SHA, Run-ID,
PASS oder FAIL und eine secretfreie Belegreferenz ergänzen. Ein fehlender
CI-Run bleibt PENDING; eine fehlende freigegebene VM bleibt BLOCKED. Beides
ist keine Abnahme.
