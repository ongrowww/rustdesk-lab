# Signierter Download und Prüfzyklen

`src/ongrow_update/runtime.rs` ist noch an keine App angebunden. Desk und Console
starten weder Prüf-Threads noch Requests. Es gibt keinen Standard-Endpunkt und
keinen Standard-Schlüssel. Ein späterer lokaler, vertrauenswürdiger Aufrufer muss
Produkt, Plattform, Kanal, öffentlichen Schlüssel, gebackene Sequenz und
kanonischen HTTPS-Ort über `RuntimePolicy::new` liefern. Die letzte Sequenz muss
der geschützte State-Provider bei jeder Prüfung frisch an `check` übergeben.
Manifest, Umgebung, GUI und Benutzerkonfiguration bestimmen diese Werte nicht.
`Unknown` beendet eine Prüfung vor Netzwerk und Sink.

## Ablauf und Grenzen

Der Aufrufer nutzt `UpdateRuntime::check` im vorhandenen Tokio-Runtime. Automatische
Prüfungen sind nach 30 Sekunden fällig, nach Erfolg erst wieder nach 24 Stunden.
Auch ein authentifiziertes, zeitlich gültiges Manifest im richtigen Kontext mit
gleicher oder älterer Sequenz ergibt `NoUpdate` und 24 Stunden Wartezeit. Dabei
werden weder Payload angefragt noch Sink-Bytes geschrieben. Ungültige Signaturen,
falscher Kontext und ungültige Zeit bleiben Fehler. Fehler und abgebrochene Futures
setzen 30 Minuten Wartezeit. Eine manuelle Prüfung
darf die Wartezeit überspringen, aber keine aktive Prüfung. Eine kurze Mutex-Sperre
schützt die Drop-Lease und liegt nie über `await`. Die Zeitplanung nutzt `Instant`,
keine veränderbare Systemuhr. Die Bibliothek startet keinen eigenen Timer-Thread.
Der spätere Startup-Aufrufer muss die Fälligkeit prüfen.

Der Client erlaubt nur HTTPS mit normaler TLS-Zertifikatsprüfung. Er folgt keinen
Redirects und fordert `Accept-Encoding: identity` an. Bestehende Proxykonventionen
bleiben erhalten. Automatische Dekompression ist ausgeschaltet. Nur HTTP 200 mit
exakt der angefragten Response-URL und ohne Content-Encoding oder mit `identity`
wird angenommen. HTTP 206, Resume und Fallback auf andere Orte sind ausgeschlossen.
Verbindung, einzelner Request und die vollständige Prüfung haben feste Grenzen von
10, 30 und 120 Sekunden. Die Gesamtfrist umfasst auch Sink-Schreiben und Flush.

Zuerst kommen die festen Dateien `manifest.json` und `manifest.sig`. Das Manifest
ist auf 64 KiB begrenzt, die detached Ed25519-Signatur auf genau 64 Bytes. Der
bestehende Verifier authentifiziert die exakten JSON-Bytes mit der Domain-Trennung
und prüft Kontext, Zeit und Sequenz. Erst danach darf ein Payload-Request starten.
Die signierte Größe darf höchstens 512 MiB betragen.

Der rohe Content-Length-Header wird vor dem Lesen auf die jeweilige Obergrenze und
bei Signatur und Payload auf die exakte erwartete Größe geprüft. Fehlt es, gelten
dieselben Grenzen für die tatsächlich empfangenen Chunks. Metadaten müssen auch
dem deklarierten Header entsprechen. Doppelte Content-Length-Header und die
Kombination mit Transfer-Encoding werden abgewiesen. Metadaten werden nur
innerhalb ihrer kleinen Grenzen gesammelt. Der Payload wird nicht vollständig
in einen Vektor kopiert. Jeder Chunk durchläuft zuerst den gemeinsamen
`StreamingPayloadVerifier`, dann den vom Aufrufer gelieferten Sink. Übergröße und
Zählerüberlauf werden vor dem Schreiben dieses Chunks abgewiesen. Am Ende müssen
Bytezahl und SHA-256 stimmen, danach muss der Sink erfolgreich flushen.

`VerifiedTransfer` bescheinigt nur diesen abgeschlossenen Transfer. Es enthält
keinen Pfad und verleiht einer später veränderten Datei keinen dauerhaften
Verified-Status. Bei jedem Fehler sind bereits geschriebene Bytes untrusted.
Der Aufrufer besitzt und entsorgt sie nach seinem geschützten Staging-Vertrag.
Dieser Download implementiert weder geschütztes Staging noch Installer-Authority,
Transaktion, Apply, Health-Prüfung oder Recovery. Er ersetzt nicht die Sitzungssperre
und beendet keine Sitzung. Ein späteres Apply muss einen geschützten Handle erneut
prüfen. Künftige Aufrufer sollen diesen Kern verwenden, keine zweite Download-Pipeline.

## Plattformen und lokale Prüfung

Der Runtime-Kern ist auf macOS und Windows nativ prüfbar. Schema 1 liefert weiterhin
nur Windows-x64-Manifeste. Auf dem Mac folgt später ein separater Sparkle-Feed und
Archivvertrag. Dieser Schritt baut keinen zweiten Mac-Installer und braucht weder
Apple-Account noch MDM-Aktion. MSI, EXE und ZIP sind für diesen Kern reine Bytes.
Eine bestandene Download-Prüfung ist keine Installer- oder Release-Abnahme.

```sh
python3 scripts/test_ongrow_update_runtime.py
python3 scripts/test_ongrow_product_profile.py
python3 scripts/test_ongrow_ci_isolation.py
python3 scripts/test_ongrow_release_signing.py --openssl /absolute/path/to/existing/openssl3
```

Die Python-Prüfung importiert die Original-Rust-Module über ein isoliertes
Cargo-Workspace unter `target`. Root-Cargo-Dateien bleiben unverändert. Auf lokalen
Macs verwendet sie den vorhandenen Rust 1.81.0 und `--offline`. Fehlende Tools oder
Cache-Einträge führen zu einem Fehler, nicht zu Installation oder grünem Skip.
Der Loopback-Server nutzt echte TLS-Requests und einen kurzlebigen CA-Schlüssel
sowie einen separat signierten Server-Leaf mit `CA:FALSE`, SAN und `serverAuth`
außerhalb Git. Die zusätzliche Test-CA existiert ausschließlich in
`cfg(test)` plus dem dedizierten Probe-Marker. Ed25519-Schlüssel entstehen im
Rust-Test nur im Speicher. Keine privaten Schlüssel werden ausgegeben oder als
Artefakt übertragen. Fehlende Testeingaben und nicht entdeckte Tests sind Fehler.

Die Fälle prüfen beide Produktrollen mit synthetischen MSI-Bytes, Chunked-Transfer
ohne Content-Length, Signatur und Hash, Zeit und Kontext, unbekannte Sequenz ohne
Request, Limits vor dem Payload, tatsächliche Unter- und Übergröße, falsches
Content-Length, Encoding, abgebrochene Streams, Gesamtfrist, TLS-Vertrauen sowie
Redirects ohne Kontakt zum Ziel. Sie zählen Requests und prüfen Sink-Bytes.
Scheduler-Tests verwenden eine Fake-Uhr; Transport-Erfolg benötigt echte Requests.
Die bestehende Manifest-Suite läuft mit. Ohne vorhandene öffentliche synthetische
Producer-Fixtures bleibt lokale OpenSSL-Interop ausdrücklich offen. CI erzeugt
sie mit dem vorhandenen OpenSSL 3 und liefert sie an die Original-Suite.

Der kleine CI-Workflow prüft `macos-14` und `windows-2022` mit exakt Rust 1.81.0,
gepinnten Actions und einem geprüften `SOURCE_SHA`. Er akzeptiert nur interne
PR-Heads oder Dispatch, hat nur `contents: read`, nutzt keine Secrets und baut
weder Flutter/VCPKG noch echte Apps. Er lädt keine TLS-Key-Artefakte hoch.
Windows-Nachweis bleibt bis zu einem tatsächlich bestandenen CI-Lauf offen.
