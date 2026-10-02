# Windows-x64-Labtest für den OnGROW Support Desk

Dieses Runbook prüft das unsigned Lab-Artefakt auf einer frischen, nicht
produktiven Windows-x64-VM. Verwende keine Kundendaten, Produktionspasswörter
oder andere Produktionssecrets. Ein erfolgreicher GitHub-Build ersetzt diesen
VM-Test nicht.

## Setup-Paket und Grenzen

Der Hauptdownload ist eine einzige Datei
`ongrow-support-desk-<version>-windows-x64-<sha8>-Setup.exe` aus dem Artefakt
`ongrow-support-desk-1.4.9-windows-x64-setup-unsigned-lab`. Daneben liegen
die `.sha256`-Datei und die `.json`-Metadaten mit Produkt `customer-desk`,
Plattform `windows-x64` und dem vollständigen Source-Commit. Diese Metadaten
sind nicht signiert und erteilen keine Freigabe für automatische Updates.

Die Setup-EXE enthält den vollständigen Desk einschließlich DLLs,
Flutter-Daten, `LICENCE` und Build-Provenienz. Sie entpackt in einen neu
angelegten eigenen Cache unter LocalAppData und startet mit `--install` den
bestehenden Installationsdialog. Auch eine umbenannte Setup-Datei öffnet diesen
Dialog. UAC, Dienst, Registry, Verknüpfungen und Deinstallation bleiben Aufgabe
des vorhandenen nativen Installationswegs. Der Wrapper wartet auf dessen
Prozessende und entfernt ausschließlich verifizierte eigene Payload-Dateien.
Unbekannte oder veränderte Cache-Reste bleiben bei einem Fehler liegen.

Die Paketierung aktiviert niemals eine Supportfreigabe. Ein erfolgreiches
Entpacken beweist keine erfolgreiche Installation und keinen laufenden Dienst.
Upgrade, Neustart, Widerruf, Identitätserhalt und Deinstallation benötigen
weiterhin die echten Windows-Tests aus Plan014. Der persönliche Windows-Rechner
ist offline und wird für diese Paketierungsarbeit nicht verwendet.

`Setup.exe --ongrow-verify-payload` prüft den eingebetteten Inhalt, entpackt ihn
sicher, liest die Dateien zurück und räumt den eigenen Cache wieder auf.
Dieser Offline-Modus startet weder Desk noch Installation, Service, Registry-
oder Enrollment-Aktionen. Die Windows-CI prüft die echte hochzuladende EXE
und dieselben Bytes unter einem anderen Dateinamen mit diesem Modus.
`ONGROW_CI_SMOKE_TEST=1` bleibt dabei gesetzt. Das ist keine menschliche
Installationsabnahme.

Das bisherige Desk-Verzeichnisartefakt
`ongrow-support-desk-1.4.9-windows-x64-unsigned-lab` bleibt als diagnostischer
Download erhalten. Es ist nicht der Kunden-Setup-Hauptweg. Die CI baut den
Desktop nur einmal mit `--skip-portable-pack`, prüft dessen Rolle und
Trust-Anchor und paketiert erst danach genau diesen geprüften Baum. Beide
Build-Jobs verwenden den exakten PR-Head-SHA. Fremde Pull Requests führen
diesen Lab-Workflow nicht aus.

## Voraussetzungen

- Frische Windows-10- oder Windows-11-x64-VM mit Snapshot vor dem Test
- Separater Testclient für die eingehende Supportverbindung
- Lab-Gerät und Testkonto in Support Control
- Setup-Artefakt `ongrow-support-desk-1.4.9-windows-x64-setup-unsigned-lab`
- Zum Setup-Dateinamen passende `.sha256`- und `.json`-Dateien
- Protokollvorlage am Ende dieses Dokuments

## Testablauf

### 1. Prüfsumme

- [ ] SHA-256 der heruntergeladenen Setup-EXE mit `Get-FileHash '<Setup-Dateiname>' -Algorithm SHA256` berechnen.
- [ ] Ergebnis mit der passenden `.sha256`-Datei und dem JSON-Feld `sha256` vergleichen.
- [ ] JSON-Felder `product`, `platform`, `upstream_version` und `source_sha`
      mit dem vorgesehenen Lab-Build vergleichen.
- [ ] Test abbrechen, wenn die Prüfsumme abweicht.

### 2. Setup-Dialog und Produktidentität

- [ ] Setup-EXE doppelklicken. Es muss der vorhandene OnGROW-Installationsdialog
      erscheinen, nicht bloß die portable Hauptoberfläche.
- [ ] Dieselbe Setup-Datei unter einem anderen Dateinamen erneut prüfen.
- [ ] Vor der Installationsbestätigung prüfen, dass keine Supportfreigabe
      automatisch aktiviert wurde.
- [ ] Dateiname, Fenstertitel, Produktname und Hersteller als OnGROW prüfen.
- [ ] Prüfen, dass nur die Kundenoberfläche erscheint und keine ausgehende
      RustDesk-Startseite angeboten wird.
- [ ] Den erwarteten Windows-SmartScreen-Hinweis für das bewusst unsigned
      Lab-Artefakt protokollieren. Schutzfunktionen nicht deaktivieren.

### 3. Installation, UAC und Service

- [ ] Installation im Setup-Dialog bewusst bestätigen und den UAC-Dialog prüfen.
- [ ] Installationspfad unter `C:\Program Files\OnGROW Support Desk` prüfen.
- [ ] In `services.msc` einen eigenen Service `OnGROW Support Desk` prüfen.
- [ ] Sicherstellen, dass eine parallel installierte normale RustDesk-App,
      deren Service und deren Uninstall-Eintrag unverändert bleiben.
- [ ] Sicherstellen, dass Installation, Privacy-Mode, Upgrade und Deinstallation
      `RuntimeBroker_rustdesk.exe` weder beenden noch ersetzen. Der
      OnGROW-Client muss `RuntimeBroker_ongrow_support_desk.exe` verwenden.

### 4. Firewall- und Netzwerkdialoge

- [ ] Alle angezeigten Windows-Firewall- oder Netzwerkdialoge protokollieren.
- [ ] Nur die für den Test benötigten privaten oder öffentlichen Netze freigeben.
- [ ] Eigene Firewall-Regeln mit dem Namen `OnGROW Support Desk Service` prüfen.

### 5. Fresh-Install ohne bestehendes OnGROW-Profil

- [ ] Vom VM-Snapshot neu starten und ausschließlich OnGROW installieren.
- [ ] Prüfen, dass keine RustDesk-Konfiguration übernommen wird.
- [ ] Prüfen, dass kein öffentlicher RustDesk-Rendezvous-Server kontaktiert wird.
- [ ] Eigenes OnGROW-Konfigurationsverzeichnis und eigene Registry-Einträge
      dokumentieren, ohne IDs oder Schlüssel in das Testprotokoll zu kopieren.

### 6. Enrollment und Heartbeat

- [ ] Enrollment auslösen und das Lab-Gerät in Support Control zuordnen.
- [ ] Prüfen, dass Support Control einen aktuellen Heartbeat empfängt.
- [ ] Nur nicht geheime Gerätebezeichnung, Zeitpunkt und Ergebnis protokollieren.

### 7. Unbeaufsichtigten Zugriff ausdrücklich aktivieren

- [ ] Den unbeaufsichtigten Zugriff in der Kundenoberfläche bewusst freigeben.
- [ ] Prüfen, dass der Zustand erst nach bestätigtem Grant als aktiv erscheint.
- [ ] Sicherstellen, dass kein permanentes Passwort angezeigt oder protokolliert wird.

### 8. Neustart und Betrieb ohne Anmeldung

- [ ] Windows vollständig neu starten.
- [ ] Vor der Benutzeranmeldung prüfen, dass der OnGROW-Service läuft.
- [ ] Nach der Anmeldung prüfen, dass Enrollment und Grant rekonstruiert werden.

### 9. Zugriff von einem getrennten Testclient

- [ ] Verbindung ausschließlich vom vorgesehenen getrennten Testclient starten.
- [ ] Bildschirmsteuerung und die freigegebenen Funktionen prüfen.
- [ ] Verbindung beenden und Sitzungsende in beiden Anwendungen prüfen.

### 10. Widerruf online und offline

- [ ] Grant bei bestehender Netzwerkverbindung widerrufen.
- [ ] Prüfen, dass eine neue unbeaufsichtigte Verbindung abgelehnt wird.
- [ ] VM auf den Ausgangssnapshot zurücksetzen und einen separaten Offline-Lauf
      vorbereiten.
- [ ] Netzwerk trennen, Widerruf beziehungsweise abgelaufenen Grant simulieren und
      prüfen, dass das zuvor gültige permanente Passwort keinen Zugriff erlaubt.
- [ ] Netzwerk wiederherstellen und die Synchronisierung mit Support Control prüfen.

### 11. Upgrade

- [ ] Eine ältere OnGROW-Labversion installieren und enrollen.
- [ ] Die neue Version darüber installieren.
- [ ] Gleiche Produktidentität, Installationspfad, Service und Uninstall-Key prüfen.
- [ ] Prüfen, ob zulässiger Enrollment-Zustand erhalten bleibt und ein widerrufener
      Grant nicht wieder aktiv wird.

### 12. Deinstallation und Restkontrolle

- [ ] OnGROW über Windows "Installierte Apps" deinstallieren.
- [ ] Prüfen, dass EXE, Installationsverzeichnis, Service, geplante Tasks,
      Firewall-Regeln, Startmenüeinträge und OnGROW-Uninstall-Key entfernt sind.
- [ ] Prüfen, dass die normale RustDesk-Installation weiterhin funktioniert.
- [ ] Verbliebene OnGROW-Konfigurationsdaten benennen. Keine Inhalte kopieren.

## Ergebnisprotokoll

Diese Felder erst nach dem echten VM-Test ausfüllen. Die Checkliste ist im
Repository absichtlich nicht als bestanden markiert.

```text
Datum:
Tester:
Windows-Version:
VM-Snapshot:
GitHub-Run:
Source-Commit:
Artefakt-SHA-256:
Schritte 1-12: OFFEN
Abweichungen:
Freigabeentscheidung: OFFEN
```

Bei einer Kollision mit RustDesk, einem Kontakt zur öffentlichen
RustDesk-Infrastruktur oder einem trotz Widerruf funktionierenden permanenten
Passwort den Test sofort abbrechen und das Artefakt nicht weitergeben.
