# Windows Support Console: Pilotabnahme

Die Windows-Konsole ist ein eigenes x64-Lab-Produkt. Das Artefakt muss
`OnGROW Support Console.exe`, `librustdesk.dll`, weitere Laufzeit-DLLs und
`data/` gemeinsam enthalten. Es ist unsigniert und nicht für den Rollout
freigegeben. Der Windows-Workflow, die isolierte Installationsprüfung und die Verbindungsprüfung
wurden lokal auf macOS nicht ausgeführt.

## Eigentum und Installation

`install_ongrow_windows_console.ps1` installiert nur unter dem aktuellen
Windows-Benutzer in `%LOCALAPPDATA%\Programs\OnGROW\Support Console`.
Das URI-Schema liegt unter
`HKCU\Software\Classes\ongrow-support-console`, der Uninstall-Eintrag unter
`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\OnGROWSupportConsole`.
Beide Einträge tragen `OnGROWOwner=de.ongrow.supportconsole`. Vorhandene
Einträge ohne diese Kennung werden nicht überschrieben. Die Deinstallation
vergleicht Kennung, Installationspfad, Uninstall-Kommando und URI-Kommando vor dem Entfernen.
Dieselben Eigentumsprüfungen gelten vor einem Upgrade. Beide Skripte prüfen
Manifestpfade und lehnen umgeleitete Dateien sowie Elternverzeichnisse ab.
Desk und Original-RustDesk besitzen andere Namen und Registry-Pfade.

Der Installer darf nur mit einer freigegebenen Testidentität und einem
vollständigen Artefakt ausgeführt werden. Upgrade, Deinstallation und
Mehrbenutzer-Eigentumstests benötigen ein isoliertes Testumfeld. Sichere
Start-, Browser- und Verbindungstests können am freigegebenen Windows-Arbeitsplatz
mit einem Testgerät stattfinden. Vor Upgrade und Deinstallation
die Konsole schließen. Keine echten Tickets, Passwörter oder Schlüssel in
Fehlerberichte oder Screenshots aufnehmen.

## Abnahmeprotokoll

Windows-Version: ausstehend. Artefakt-SHA: ausstehend. CI-Run-ID: ausstehend.
Test-Subject und Testgerät: nur lokal im freigegebenen Pilot dokumentieren.

| Fall | Erwartung | Ergebnis |
| --- | --- | --- |
| W01 | Desk, Original-RustDesk und Konsole bleiben getrennt startbar | PENDING: Windows-Praxistest |
| W02 | Benutzer A registriert die Konsole; Benutzer B kann A-Schlüssel nicht lesen | PENDING: isoliertes Mehrbenutzer-Testumfeld |
| W03 | Browser-URI registriert startet die kalte Konsole genau einmal | PENDING: Windows-Praxistest |
| W04 | Browser-URI launch erreicht die bereits laufende Konsole genau einmal | PENDING: Windows-Praxistest |
| W05 | Gültiges Ticket startet eine echte Verbindung zum Testgerät; ACK erst nach erfolgreicher nativer Authentifizierung über `LoginResponse`, niemals bei Transport-`connection_ready` | PENDING: Windows-Praxistest |
| W06 | Replay, Ablauf und fremdes Ticket erzeugen keine Verbindung | PENDING: Windows-Praxistest |
| W07 | Sperre, Widerruf und Rollenentzug verhindern neue Verbindungen | PENDING: Windows-Praxistest |
| W08 | Benutzerneustart erhält die registrierte Identität | PENDING: Windows-Praxistest |
| W09 | Upgrade erhält Identität und URI-Eigentum | PENDING: isoliertes Installationstestumfeld |
| W10 | Deinstallation entfernt nur Console-Einträge und Console-Dateien | PENDING: isoliertes Installationstestumfeld |
| W11 | Sentinel erscheint weder in Logs noch UI-Fehlern oder Prozessargumenten | PENDING: Windows-Praxistest |
| W12 | Windows-Startup-Test bestätigt initialisierte, sichtbare Console-UI nach einem nativen gerenderten Frame | PENDING: Windows-CI |

Für jeden Fall nach Durchführung Windows-Version, finalen Git-SHA, Run-ID,
PASS oder FAIL und eine secretfreie Belegreferenz ergänzen. Ein fehlender
CI-Run oder Praxistest bleibt PENDING. Lokale statische und gemockte Tests
ersetzen weder einen Windows-Build noch diese Abnahme.
