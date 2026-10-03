# Geschützter Sequenzzustand und fester Stage-Slot

Der Store ist an keine echte App angebunden. Die explizite crate-interne
Bootstrap-API erwartet einen bereits vorhandenen, geschützten Produktroot.
Sie erzeugt weder Verzeichnisse noch ACLs und repariert keine vorhandenen Dateien.
Produkt, Schema-1-Plattform, Kanal, gebackene Sequenz, öffentlicher Schlüssel und
HTTPS-Policy stammen gemeinsam aus der privaten vertrauenswürdigen Identität.
Es gibt keine ENV-/GUI-Konfiguration, Acceptance-Advance- oder Recovery-API.

`accepted-sequence-v1` hat exakt 56 Bytes mit Version, Identität, Sequenz und
SHA-256-Prüfsumme. Die Prüfsumme erkennt zufällige oder partielle Schäden, sie
authentisiert nicht. Vertrauen entsteht durch die bestehenden OS-Owner-, ACL-,
Typ-, Linkzahl-, lokalen Dateisystem- und gehaltenen Handleprüfungen.
Der ein Byte lange `staging-v1.lock` enthält Version 1. Fehlende, partielle oder
ungültige Dateien blockieren Snapshot und Download vor jedem Request. Bootstrap
ist ausschließlich erstmalig und überschreibt nichts. Ein Download oder Seal
ändert die akzeptierte Sequenz nicht.

Die drei neuen Namen sind feste interne Auswahlen. Manifestfilename und Caller
liefern keine Pfadautorität. macOS öffnet relativ zum gehaltenen Root mit
`openat` und `O_NOFOLLOW`; Windows hält die geprüften Vorfahren ohne Delete-Sharing
und öffnet mit `OPEN_REPARSE_POINT`. Bestehende Gate-Prüffunktionen bleiben
byteidentisch außerhalb jeweils eines markierten Additionsblocks. Kein zweiter
ACL-Parser entscheidet über Vertrauen.

Der eigene nichtblockierende Kernel-Lock ist unabhängig von Sessionadmission
und Pending. Windows hält einen Read-only-Lockhandle ohne Write-/Delete-Sharing.
Die exklusive Sperre liegt bei Offset 1, Länge 1, jenseits des Versionbytes.
Andere Snapshot-Handles dürfen deshalb Byte 0 lesen, während alle Stageprozesse
auf denselben Lockbereich konkurrieren. Siehe [LockFileEx-Vertrag](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex).

`begin_stage` prüft gesunden Zustand, nimmt die Stagelease und weist vorhandene
`stage-v1.payload` vor Netzwerk ab. Es erzeugt noch keine Datei. Der private
AsyncWrite-Sink erzeugt sie ausschließlich beim ersten tatsächlichen Payloadwrite
mit `create_new` oder `O_EXCL`. `NotDue`, authentifiziertes `NoUpdate` und reine
Metadatenfehler lassen keinen Slot und senden keinen Payloadrequest.
Die vorhandene Runtime ist die einzige Downloadpipeline. Jedes Datei-I/O läuft
im vorhandenen Tokio-Blockingpool mit eigenem Root- und Stagelease-Besitz.
Pro Write werden höchstens 64 KiB kopiert. Auch nach Caller-Cancel bleiben diese
Guards bis zum letzten Job-I/O und zum Schließen eines verworfenen Outputs erhalten.

Seal übernimmt den exklusiven Writer erst nach abgeschlossenem Job, flusht und
synchronisiert ihn und schließt ihn vor dem geprüften Read-only-Reopen. Der
neu geöffnete Handle muss dieselbe Dateiidentität haben. Frischer Snapshot und
frische Zeit fließen mit dem privaten Storekey in die erneute Originalmanifest-
Verifikation. Der neu erzeugte Candidate prüft Größe und SHA-256 der tatsächlichen
Handlebytes. Ein Ticket besitzt diesen Read-only-Handle, Originalmanifest und
Signatur, Root und Stagelease. Es exportiert weder Pfad noch Writehandle oder
Installerprivileg. Seine spätere Verwendung verlangt erneute Handleverifikation.

macOS nutzt Dateisynchronisation plus `F_FULLFSYNC` und synchronisiert den
Verzeichnishandle. Windows erzeugt mit `WRITE_THROUGH` und flusht Datei-Handles.
Die anschließende Read-only-Rootinspektion ist kein bewiesener Powercut-
Durability-Nachweis für Verzeichniseinträge. Ein partieller Bootstrap bleibt
unbenutzbar, statt fehlende Dateien nachzubauen.

Es gibt keine produktive Löschung, weder bei Drop noch nach Seal, Cancel oder
Fehler. Ein einmal erzeugter Slot bleibt belegt und untrusted bei Fehlern.
Das begrenzt Dateiwachstum auf einen Slot, liefert aber noch keinen Retry- oder
Recoveryvertrag. Der isolierte Harness entfernt nur seine eigenen temporären
Fixtures. Ein Folgepaket muss Discard und persistente Recovery definieren, bevor
Apps diese API aktivieren.

## Native Prüfung und offene Grenzen

`python3 scripts/test_ongrow_update_store.py` importiert die Originalmodule mit
den vorhandenen gepinnten Runtime-Fixture-Abhängigkeiten. Lokal verwendet es
gecachten Rust 1.81.0 und Offline-Cargo. `--source-only` prüft die vier exakten
Originalhashes, feste Namen, private Grenzen und CI-Vertrag. Fehlende Tools,
Testeingaben oder nicht entdeckte Fälle sind Fehler, keine grünen Skips.

Echte Loopback-TLS-Requests durchlaufen die unveränderte Runtime. Temporäre
CA/Leaf-Schlüssel entstehen außerhalb Git und werden weder geloggt noch als
Artefakte übertragen. Console-Manifeste signiert ein frischer Ed25519-Key im
Rust-Testprozess. Die CustomerDesk-Public-Producer-Fixtures bleiben eine getrennte
Runtime-Interop-Regression und werden niemals in den Console-Store umetikettiert.

Die native Store-Probe verwendet ausschließlich SupportConsole im Userroot.
CustomerDesk im Console-Fixture-Root muss vor jedem Dateieffekt scheitern.
Privilegierte Desk-/SYSTEM- und echte Cross-Principal-Prüfungen bleiben offen.
macOS-Read-only und advisory flock verhindern keine Mutation durch denselben
vertrauenswürdigen Eigentümer. Die Probe prüft diese Grenze durch tatsächliche
Mutation und anschließende Hash-Ablehnung. Windows muss Write und Replace während
des gehaltenen Read-only-Tickets verweigern. Lokales macOS-PASS ist kein
Windows-PASS; beide nativen CI-Runner folgen erst nach Review.

Der Gesamtupdater bleibt unvollständig. Installer-Authority, Exclusive/Pending
vor Prozessstop, Installer, Zielprozess, Health, Rollback und Aktivierung fehlen.
