# Plan 007: Autorisierte Technikerverbindung ohne Passwortanzeige

Status: TODO
Priorität: Hoch
Abhängigkeiten: Plan 004, Plan 005 und Plan 006
Geplant am: 2026-07-30

## Ziel

Ein berechtigter OnGROW-Supportmitarbeiter kann aus dem Admin-Inventar eine
Verbindung zu einem freigegebenen Gerät starten, ohne das permanente Passwort
im Browser, in einer URL, in der Zwischenablage oder in einer lokalen Datei zu
sehen. Jeder Verbindungsversuch ist kurzlebig, an einen registrierten
Technikerclient gebunden, einmalig nutzbar und auditierbar.

## Ausgangslage

- Plan 004 authentifiziert Mitarbeitende per OIDC und unterscheidet mindestens
  die Rollen `admin` und `support`.
- Plan 005 verwahrt permanente Passwörter verschlüsselt und liefert sie nie an
  den Browser.
- Plan 006 erzeugt einen Grant nur nach ausdrücklicher Zustimmung des Kunden.
- Die RustDesk-ID ist änderbar. Alle Autorisierungsentscheidungen müssen daher
  den unveränderlichen Geräte-Fingerprint verwenden.

## Architekturentscheidung

Der Browser erhält ausschließlich eine undurchsichtige, kurzlebige
Verbindungsanforderung. Ein separat registrierter OnGROW-Technikerclient
besitzt ein gerätegebundenes Schlüsselpaar. Das Control Plane entschlüsselt
das gespeicherte Secret nur kurz im Arbeitsspeicher und versiegelt es direkt
für den öffentlichen Schlüssel dieses Technikerclients.

Ein Custom-URL-Schema darf höchstens die zufällige ID einer einmaligen
Anforderung transportieren. Weder Passwort, Ciphertext noch wiederverwendbarer
Bearer-Token gehören in die URL. Vor Verwendung wird das bereits vorhandene
OnGROW-Schema und dessen Plattformregistrierung geprüft.

## Umsetzungsschritte

### 1. Technikerclients registrieren

- Im Control Plane ein Modell für registrierte Technikergeräte mit
  öffentlichem Signatur- und Verschlüsselungsschlüssel anlegen.
- Registrierung nur nach erfolgreicher OIDC-Anmeldung und ausdrücklicher
  Bestätigung durch einen `admin` erlauben.
- Registrierung an OIDC-Subject, Organisation, Gerätename, Key-Fingerprint,
  Erstellungszeitpunkt und Status binden.
- Verlust, Sperrung und Schlüsselrotation unterstützen.
- Private Schlüssel ausschließlich im sicheren Betriebssystemspeicher des
  Technikergeräts ablegen, zum Beispiel macOS Keychain oder Windows
  Credential Protection.

### 2. Kurzlebige Connection Grants modellieren

- Tabelle und Zustandsmaschine für `connection_grants` anlegen.
- Grant an Kunden-Gerätefingerprint, Mitarbeiter-Subject,
  Technikerclient-Key, Zweck, Erstellungszeitpunkt und Request-ID binden.
- Lebensdauer auf höchstens 60 Sekunden begrenzen.
- Grant nach erfolgreicher Einlösung atomar als verbraucht markieren.
- Abgelaufene, widerrufene, doppelt eingelöste oder an einen anderen
  Technikerclient gerichtete Grants ablehnen.
- Nur Geräte mit aktivem Grant aus Plan 006 zulassen.

### 3. Admin-Aktion implementieren

- In der Geräteansicht die Aktion „Supportverbindung starten“ nur für
  berechtigte Rollen und freigegebene Geräte aktivieren.
- Vor Erzeugung Zielgerät, Kundenorganisation und Zugriffsstatus deutlich
  bestätigen lassen.
- Das Control Plane erzeugt nur die undurchsichtige Connection-Grant-ID für
  den Browser.
- Die lokale App über das registrierte URL-Schema oder einen kleinen
  Launcher öffnen; URL nur mit der einmaligen Grant-ID versehen.
- Browser-Responses, History, Analytics und Fehlertexte dürfen kein Secret
  und keinen verschlüsselten Secret-Envelope enthalten.

### 4. Technikerclient sicher anbinden

- Im OnGROW-Client einen klar getrennten Techniker-Modus beziehungsweise
  Launcher implementieren; Kundenmodus darf keine Admin-Funktionen erhalten.
- Die Connection-Grant-ID über einen authentifizierten Kanal einlösen.
- Request mit dem registrierten Techniker-Schlüssel signieren.
- Das Control Plane prüft OIDC-/Gerätebindung, Rolle, Ablauf,
  Einmalverwendung und aktuellen Kunden-Grant.
- Das Secret serverseitig nur im Speicher entschlüsseln und unmittelbar für
  den Technikerclient versiegeln.
- Der Technikerclient entschlüsselt nur im Speicher und übergibt das Passwort
  an den vorhandenen nativen RustDesk-Verbindungsablauf.
- Nach Verbindungsstart Secret-Puffer soweit von den verwendeten Bibliotheken
  unterstützt explizit leeren.

### 5. Audit und Missbrauchsschutz ergänzen

- Audit-Ereignisse für Anforderung, Freigabeprüfung, Zustellung, Einlösung,
  Ablehnung, Ablauf, Verbindungsauslösung und Client-Sperrung schreiben.
- Rate Limits pro Mitarbeiter, Technikerclient und Zielgerät anwenden.
- Auffällige Wiederholungen und abgelehnte Einlösungen messbar machen.
- Eine laufende oder zukünftige Verbindung sofort blockieren, wenn der Kunde
  den Grant widerruft oder der Technikerclient gesperrt wird.
- Keine Desktop-Vorschau oder Screenshots erfassen; das wäre ein eigener,
  gesondert zu bewertender Funktionsumfang.

### 6. Plattformintegration absichern

- Custom-URL-Schema auf macOS und Windows gegen fremde Handler,
  Parameterinjektion und mehrfaches Öffnen testen.
- Ausschließlich streng validierte zufällige Grant-IDs akzeptieren.
- Falls die URL-Schema-Absicherung auf einer Plattform nicht hinreichend
  zuverlässig ist, einen localhost-Callback mit Proof-of-Possession oder eine
  andere betriebssystemspezifische, dokumentierte Übergabe einsetzen.
- Linux erst im Produktionsplan nachziehen, sofern der Pilot auf macOS und
  Windows stabil ist.

## Tests und Verifikation

- Tests mit zwei Mitarbeitenden und zwei registrierten Technikerclients:
  falscher Client darf einen fremden Grant nicht einlösen.
- Ablauf-, Replay-, Doppel-Klick-, Rollenwechsel- und
  Client-Sperrungstests.
- Test, dass ein zwischenzeitlicher Kunden-Widerruf die Einlösung verhindert.
- Tests gegen URL-Manipulation und fremde Scheme-Handler.
- End-to-End-Test vom Admin-Klick bis zum nativen Verbindungsstart mit
  Testgerät und Testsecret.
- Automatisierte Prüfung von Browser-Responses, URLs, Logs, Auditdaten,
  Zwischenablage und temporären Dateien auf Secret-Leaks.
- Sicherheitsreview des vollständigen Datenflusses vor Pilotfreigabe.

## Dokumentation

- Registrierung, Sperrung und Verlust eines Technikergeräts dokumentieren.
- Support-Runbook für abgelaufene Grants, offline Geräte und widerrufene
  Freigaben anlegen.
- Audit-Auswertung und Eskalationsweg bei verdächtigen Zugriffen festhalten.
- Klarstellen, dass eine Gerätefreigabe keine automatische Berechtigung für
  jeden Mitarbeiter bedeutet.

## Abnahmekriterien

- Das Passwort erscheint niemals im Browser, in URLs, Zwischenablage, Logs
  oder Dateien.
- Nur registrierte Technikerclients berechtigter Mitarbeitender können einen
  Grant einlösen.
- Connection Grants sind höchstens 60 Sekunden gültig und atomar einmalig.
- Kundenwiderruf und Technikerclient-Sperrung greifen vor einer neuen
  Verbindung.
- Jeder Versuch ist mit Akteur, Ziel, Ergebnis und Request-ID auditierbar.
- Der End-to-End-Test funktioniert auf dem für den Pilot ausgewählten
  Betriebssystem.

## STOP

- Keine allgemeine Passwort-Download- oder Copy-Funktion als Abkürzung bauen.
- Keine Secrets oder langlebigen Tokens in Custom URLs übertragen.
- Kein Rollout an Supportmitarbeitende ohne registrierte, sperrbare
  Technikerclients und abgeschlossenes Sicherheitsreview.
- Kein Produktivdeployment ohne erneute, ausdrückliche Freigabe.
