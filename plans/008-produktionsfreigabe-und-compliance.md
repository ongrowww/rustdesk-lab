# Plan 008: Produktionsfreigabe, Wiederherstellung und Compliance absichern

Status: TODO
Priorität: Hoch
Abhängigkeiten: Plan 002 bis Plan 007
Geplant am: 2026-07-30

## Ziel

Die bisherige Lern- und Pilotarchitektur wird erst nach überprüfbaren
Sicherheits-, Betriebs-, Datenschutz-, Lizenz- und Plattform-Gates für echte
Kundengeräte freigegeben. Der Plan trennt ausdrücklich einen erfolgreichen
technischen Prototyp von einem belastbaren Supportprodukt.

## Ausgangslage

- Client und Server sind dünne Forks von RustDesk und stehen unter AGPL-3.0.
- Die bisherigen macOS-Artefakte dienen dem internen Test und ersetzen keine
  vollständige Developer-ID-Signierung und Apple-Notarisierung.
- Plan 002 bis 007 führen eigene Protokollfelder, ein Control Plane,
  Geräteinventar, Secrets, Zustimmung und Technikerzugriff ein.
- Das System verarbeitet sicherheitskritische Zugangsdaten und
  Geräte-/Kundenzuordnungen. Ein normaler „Build ist grün“-Nachweis reicht
  deshalb nicht für Produktion.

## Umsetzungsschritte

### 1. Architektur und Bedrohungsmodell unabhängig prüfen

- Alle Datenflüsse und Vertrauensgrenzen aus Plan 002 bis 007 konsolidieren.
- Ein unabhängiges Security Review mit Schwerpunkt
  Authentifizierung, Autorisierung, Kryptografie, Replay,
  Schlüsselverwaltung, Client-Update und Supply Chain durchführen.
- Kritische und hohe Findings vor jedem Kundenpilot schließen.
- Für verbleibende Risiken benannte Verantwortliche, Frist und dokumentierte
  Akzeptanz verlangen.
- Einen gezielten Penetrationstest der extern erreichbaren APIs und
  Techniker-Workflows einplanen.

### 2. Lizenz- und Veröffentlichungspflichten klären

- Änderungen an RustDesk-Client und -Server als AGPL-3.0-Forks behandeln.
- Einen konkreten Prozess für vollständigen korrespondierenden Quellcode,
  Buildanweisungen, Lizenztexte, Copyright-Hinweise und Zugriff auf die
  tatsächlich ausgelieferte Version festlegen.
- Verwendete Drittbibliotheken und Assets einschließlich Icons,
  Schriftarten und Kryptobibliotheken inventarisieren.
- Automatisierte SBOM- und Lizenzprüfung in CI ergänzen.
- Vor kommerzieller Nutzung eine qualifizierte rechtliche Prüfung einholen;
  dieser Plan ist keine Rechtsberatung.

### 3. Datenschutz und Einwilligung operationalisieren

- Zweck, Rechtsgrundlage, Rollen, Aufbewahrung und Löschung der
  Geräte-/Auditdaten mit Datenschutzverantwortlichen festlegen.
- Zustimmungstext und Widerrufsweg aus Plan 006 rechtlich und sprachlich
  prüfen.
- Datenminimierung durchsetzen: keine Desktop-Vorschauen, Screenshots,
  Zwischenablageinhalte oder Sitzungsaufzeichnungen standardmäßig erfassen.
- Lösch- und Auskunftsprozesse für Kundenorganisationen und Geräte
  implementieren und testen.
- Auftragsverarbeitung, Verzeichnis von Verarbeitungstätigkeiten und eine
  gegebenenfalls erforderliche Datenschutz-Folgenabschätzung prüfen.

### 4. Produktionsfähige Identität und Infrastruktur

- Einen produktionsgeeigneten OIDC-Provider mit MFA,
  Offboarding und kurzen Sitzungen konfigurieren.
- Admin- und Supportrollen nach Least Privilege vergeben und regelmäßig
  rezertifizieren.
- Netzwerkzugriffe zwischen Reverse Proxy, hbbs/hbbr, Control Plane und
  Datenbank minimieren.
- TLS, Security Header, Rate Limits, Zeit-Synchronisierung und restriktive
  Firewall-Regeln prüfen.
- Container als nicht privilegierte Nutzer mit schreibgeschütztem Root-Dateisystem
  und expliziten Volumes betreiben.
- Produktiv-, Staging- und Entwicklungsdaten sowie Schlüssel vollständig
  trennen.

### 5. Backup, Restore und Disaster Recovery

- Datenbank-Backups verschlüsseln und getrennt vom Master-Key sichern.
- Schlüssel-Backups mit Mehrpersonenfreigabe beziehungsweise gleichwertiger
  organisatorischer Kontrolle schützen.
- Regelmäßige Restore-Tests in einer isolierten Umgebung durchführen.
- Recovery Time Objective und Recovery Point Objective festlegen.
- Ausfall von Control Plane, hbbs/hbbr, OIDC und DNS einzeln üben.
- Einen dokumentierten Notfallweg zum Sperren aller Technikerclients und
  Widerrufen aller Grants bereitstellen.

### 6. Monitoring und Incident Response

- Metriken für Enrollment-Fehler, Replay-Ablehnungen, Secret-Operationen,
  Connection Grants, fehlgeschlagene Logins und Schlüsselzustand erfassen,
  ohne Secrets oder unnötige personenbezogene Inhalte zu loggen.
- Alarmgrenzen und Rufbereitschaft/Eskalationskontakte festlegen.
- Audit-Aufbewahrung manipulationsarm gestalten und Zugriff darauf gesondert
  beschränken.
- Runbooks für kompromittierte Mitarbeiterkonten, verlorene
  Technikergeräte, geleakte Schlüssel, manipulierte Builds und
  Serverkompromittierung testen.
- Post-Incident-Prozess und Kundenkommunikation vorbereiten.

### 7. Vertrauenswürdige Builds und Updates

- Reproduzierbare beziehungsweise nachvollziehbare, gepinnte CI-Abhängigkeiten
  und Provenance für Client- und Serverartefakte anstreben.
- macOS mit Apple Developer ID signieren und notarisieren.
- Windows-Installer mit geeignetem Code-Signing-Zertifikat signieren.
- Update-Metadaten kryptografisch signieren, Rollback und
  kompromittierte Update-Schlüssel berücksichtigen.
- Fork-Synchronisierung mit Upstream weiterhin als kleine, überprüfbare
  Patchserie betreiben.
- Abhängigkeitsscans und gezielte Regressionstests für RustDesk-Upgrades
  einführen.

### 8. Plattformen gestuft freigeben

- Zuerst einen internen macOS-Test mit nicht produktiven Geräten durchführen.
- Danach einen eng begrenzten, ausdrücklich vereinbarten Kundenpilot mit
  Rückfall- und Widerrufsplan.
- Windows-Berechtigungen, Serviceinstallation, Credential-Speicher,
  Signierung und Deinstallation separat abnehmen.
- Linux wegen unterschiedlicher Desktop-, Display-Server- und
  Paketvarianten als eigenen Plattform-Track behandeln.
- Keine Plattform nur aufgrund eines erfolgreichen Cross-Compile-Builds als
  unterstützt kennzeichnen.

### 9. Go-live-Gate durchführen

- Eine versionierte Checkliste mit Verantwortlichen und Nachweisen führen.
- Go-live erfordert mindestens:
  - keine offenen kritischen oder hohen Security Findings,
  - bestandenen Restore- und Key-Rotation-Test,
  - getesteten Kundenwiderruf und globalen Notfallstopp,
  - signierte/notarisierte Artefakte,
  - geklärte Lizenz- und Datenschutzpflichten,
  - getestetes Offboarding eines Supportmitarbeiters,
  - dokumentierte Betriebsverantwortung.
- Jede Freigabe auf konkrete Client-, Server- und Control-Plane-Versionen
  beziehen.

## Tests und Verifikation

- Vollständiger End-to-End-Test auf jeder freigegebenen Plattform.
- Restore-Drill, Schlüsselrotation, OIDC-Ausfall, Control-Plane-Ausfall und
  globaler Widerruf.
- Externes Security Review und Penetrationstest mit dokumentierter
  Nachverfolgung.
- SBOM-, Lizenz-, Secret-, Dependency- und Artefaktsignaturprüfung in CI.
- Pilotbeobachtung mit definierten Erfolgs- und Abbruchkriterien.
- Deinstallations- und Datenlöschungstest.

## Dokumentation

- Betriebs-, Sicherheits-, Restore-, Incident- und Offboarding-Runbooks
  vervollständigen.
- Unterstützte Plattformen und bekannte Einschränkungen veröffentlichen.
- Kundenverständlich erklären, wann Zugriff möglich ist, wie er protokolliert
  wird und wie er widerrufen werden kann.
- Quellcode- und Lizenzhinweise passend zu den ausgelieferten Versionen
  bereitstellen.

## Abnahmekriterien

- Das Go-live-Gate ist mit konkreten Nachweisen und Verantwortlichen
  vollständig.
- Artefakte sind auf der jeweiligen Plattform vertrauenswürdig signiert.
- Restore, Rotation, Widerruf, Offboarding und Notfallstopp wurden praktisch
  getestet.
- Datenschutz- und AGPL-Pflichten sind fachkundig geprüft und operativ
  umgesetzt.
- Monitoring und Incident Response sind vor dem ersten Kundenpilot aktiv.
- Nur ausdrücklich freigegebene Plattformen werden als unterstützt
  bezeichnet.

## STOP

- Kein Kundenrollout allein auf Basis eines erfolgreichen Prototyps.
- Keine produktiven Passwörter oder Schlüssel in CI-Variablen, Plänen,
  Tickets oder Logs hinterlegen.
- Keine Abschwächung von Gatekeeper, SIP, Firewall, MFA oder
  Code-Signing-Anforderungen zur Vereinfachung.
- Keine rechtlichen Schlussfolgerungen aus diesem technischen Plan ableiten;
  fachkundige Prüfung ist ein verbindliches Gate.
