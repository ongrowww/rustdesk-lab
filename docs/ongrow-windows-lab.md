# Windows-x64-Labtest für den OnGROW Support Desk

Dieses Runbook prüft das unsigned Lab-Artefakt auf einer frischen, nicht
produktiven Windows-x64-VM. Verwende keine Kundendaten, Produktionspasswörter
oder andere Produktionssecrets. Ein erfolgreicher GitHub-Build ersetzt diesen
VM-Test nicht.

## Voraussetzungen

- Frische Windows-10- oder Windows-11-x64-VM mit Snapshot vor dem Test
- Separater Testclient für die eingehende Supportverbindung
- Lab-Gerät und Testkonto in Support Control
- Artefakt `ongrow-support-desk-1.4.9-windows-x64-unsigned-lab`
- Prüfsummendatei `OnGROW Support Desk.exe.sha256`
- Protokollvorlage am Ende dieses Dokuments

## Testablauf

### 1. Prüfsumme

- [ ] SHA-256 der EXE mit `Get-FileHash '.\OnGROW Support Desk.exe' -Algorithm SHA256` berechnen.
- [ ] Ergebnis mit `OnGROW Support Desk.exe.sha256` vergleichen.
- [ ] Test abbrechen, wenn die Prüfsumme abweicht.

### 2. Portable App und Produktidentität

- [ ] EXE zunächst ohne Installation starten.
- [ ] Dateiname, Fenstertitel, Produktname und Hersteller als OnGROW prüfen.
- [ ] Prüfen, dass nur die Kundenoberfläche erscheint und keine ausgehende
      RustDesk-Startseite angeboten wird.
- [ ] Den erwarteten Windows-SmartScreen-Hinweis für das bewusst unsigned
      Lab-Artefakt protokollieren. Schutzfunktionen nicht deaktivieren.

### 3. Installation, UAC und Service

- [ ] Installation aus der App starten und den UAC-Dialog prüfen.
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
