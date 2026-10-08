# Geschützter Sequenzzustand und fester Stage-Slot

Der Store ist an keine echte App angebunden. Die explizite crate-interne
Bootstrap-API erwartet einen bereits vorhandenen, geschützten Produktroot.
Sie erzeugt weder Verzeichnisse noch ACLs und repariert keine vorhandenen Dateien.
Produkt, Schema-1-Plattform, Kanal, gebackene Sequenz, öffentlicher Schlüssel und
HTTPS-Policy stammen gemeinsam aus der privaten vertrauenswürdigen Identität.
Es gibt keine ENV-/GUI-Konfiguration oder Acceptance-Advance-API. Die unten
beschriebene Wiederaufnahme prüft nur einen bestehenden Download.

`accepted-sequence-v1` hat exakt 56 Bytes mit Version, Identität, Sequenz und
SHA-256-Prüfsumme. Die Prüfsumme erkennt zufällige oder partielle Schäden, sie
authentisiert nicht. Vertrauen entsteht durch die bestehenden OS-Owner-, ACL-,
Typ-, Linkzahl-, lokalen Dateisystem- und gehaltenen Handleprüfungen.
Der ein Byte lange `staging-v1.lock` enthält Version 1. Fehlende, partielle oder
ungültige Dateien blockieren Snapshot und Download vor jedem Request. Bootstrap
ist ausschließlich erstmalig und überschreibt nichts. Ein Download oder Seal
ändert die akzeptierte Sequenz nicht.

Die vier Store-Namen sind feste interne Auswahlen. Manifestfilename und Caller
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
`stage-v1.payload` oder `attempt-v1.signed` vor Netzwerk ab. Es erzeugt noch keine
Datei. Der private
AsyncWrite-Sink erzeugt sie ausschließlich beim ersten tatsächlichen Payloadwrite
mit `create_new` oder `O_EXCL`. `NotDue`, authentifiziertes `NoUpdate` und reine
Metadatenfehler lassen keinen Slot und senden keinen Payloadrequest.
Die vorhandene Runtime ist die einzige Downloadpipeline. Jedes Datei-I/O läuft
im vorhandenen Tokio-Blockingpool mit eigenem Root- und Stagelease-Besitz.
Pro Write werden höchstens 64 KiB kopiert. Auch nach Caller-Cancel bleiben diese
Guards bis zum letzten Job-I/O und zum Schließen eines verworfenen Outputs erhalten.
Create/Write/Flush und Seal nutzen private Job-Factories mit Writer-first-Input.
Die erste Closure-Anweisung übernimmt den ganzen Container lokal. Das erzwingt
Whole-Capture statt einzelner Edition-2021-Feldcaptures, deren Drop-Reihenfolge
keinen Writer-vor-Guards-Vertrag liefert. Auch ein vor seinem Start verworfener
Job schließt dadurch den Writer vor Stagelease und Root.

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
Das begrenzt Dateiwachstum auf einen Slot. Die Wiederaufnahme eines vollständig
signierten Downloads ist unten beschrieben. Discard beschädigter Slots und
Recovery einer begonnenen Installation fehlen weiterhin. Der isolierte Harness
entfernt nur seine eigenen temporären Fixtures.

## Native Prüfung und offene Grenzen

`python3 scripts/test_ongrow_update_store.py` importiert die Originalmodule mit
den vorhandenen gepinnten Runtime-Fixture-Abhängigkeiten. Lokal verwendet es
gecachten Rust 1.81.0 und Offline-Cargo. `--source-only` prüft die vier exakten
Originalhashes, feste Namen, private Grenzen und CI-Vertrag. Fehlende Tools,
Testeingaben oder nicht entdeckte Fälle sind Fehler, keine grünen Skips.

Die vier bytegeprüften Quellen `session_gate.rs`, `session_gate/macos.rs`,
`session_gate/windows.rs` und `runtime.rs` unter `src/ongrow_update/` haben
explizit `text eol=lf` in `.gitattributes`. Die übrigen Quellen behalten
`* text=auto`. Der Harness prüft weiter tatsächliche Worktreebytes, ohne
Zeilenenden zu normalisieren oder die Originalhashes zu ändern.
Eine isolierte Git-Fixture mit `core.autocrlf=true` verwendet die vier
kanonischen HEAD-Blobs und die aktuellen Worktree-Attribute. Ein neuer
Checkout muss diese Quellen byteidentisch mit LF erzeugen, während eine
neutrale Kontrolldatei CRLF erhält. Künstliche CRLF-Quellen, Byteänderungen
außerhalb der Addition sowie doppelte oder fehlende Marker müssen scheitern.
Beide CI-Workflows reagieren auch auf `.gitattributes` und diesen Source-Harness.
Der lokale Checkout-Test ersetzt keine neue native Windows-CI-Matrix für
denselben geprüften Commit.

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

Die Ownership-Probe verwendet genau dieselben Job-Closures wie Produktion.
Sie verwirft Write- und Seal-Closure vor dem Aufruf, ohne laufenden Tokio- oder
Client-Hintergrund. Seal erhält einen echten Transfer aus dem Original-TLS-Pfad.
Ein früher Sealfehler nutzt die tatsächliche Sequence-Dateiidentität als falsche
Payloadidentität. Ein ausschließlich Testmarker-gebundener Beobachter hält nur
Weak-Referenzen und einen nichtbesitzenden FD-/Handlewert. Er prüft zuerst
`F_GETFD`/`EBADF` oder `GetHandleInformation`/`ERROR_INVALID_HANDLE`, ohne vorher
ein neues Handle zu öffnen. Danach muss der Root noch leben und ein echter
Childprozess am Stage-Lock scheitern, während Versionbyte 0 lesbar bleibt.
Nach vollständig verworfenem Job muss derselbe Child den Lock erhalten; Slot
und akzeptierte Sequenz bleiben unverändert. Das ist ein nativer Beleg für
Closure-Ownership vor Jobstart und beim frühen Fehler, kein Nachweis tatsächlichen
Tokio-Shutdown-Schedulings. Der gesonderte echte Creating-Cancel-Test bleibt bestehen.

Der Gesamtupdater bleibt unvollständig. Installer-Authority, Exclusive/Pending
vor Prozessstop, Installer, Zielprozess, Health, Rollback und Aktivierung fehlen.
## Dauerhaft gespeicherter signierter Download

Nach Flush, erneuter Signaturprüfung und vollständiger Hashprüfung der Payload
schreibt der Store `attempt-v1.signed`. Die größenbegrenzte Datei enthält nur
Version, Länge, ursprüngliche Signatur und die exakten signierten Manifestbytes.
Erstellung, Synchronisation und Read-only-Reopen verwenden die bestehenden
nativen Root-, Owner-, ACL- und Linkprüfungen. Vorhandene oder partielle Dateien
werden niemals ersetzt.

`ProtectedStore::resume_stage` öffnet diese Datei und die feste Payload unter
derselben prozessübergreifenden Stagelease erneut. Vertrauenswürdige
Installationsidentität, frischer Sequenzzustand und aktuelle Zeit fließen in den
ursprünglichen Signaturprüfer und die vollständige Payloadprüfung. Erst danach
entsteht ein neues Sealed-Ticket. Es gibt keine HTTP-Requests. Auch eine spätere
Ticketprüfung liest die gehaltene Manifestdatei erneut. Fehlende, beschädigte,
abgelaufene oder wiederholte Updates sowie falsches Produkt, Schlüssel oder Kanal
bleiben blockiert. Fehler ändern weder Dateien noch akzeptierten Zustand.

Das ist Download-Wiederaufnahme, keine Installations-Recovery. Die API erzeugt
keinen Root, startet keinen Installer, löscht kein Pending, bescheinigt keine
Health, erhöht keine Sequenz und entfernt keine Dateien. App-/Dienst-Timer,
Bootstrap, externer Installer-Guardian und authentifizierte Prüfung der neuen
Installation sind vor Aktivierung des Autoinstallers weiterhin erforderlich.

Der native Test führt die Wiederaufnahme in einem separaten Prozess aus, nachdem
alle Downloadhandles geschlossen sind. Er prüft ausbleibende Requests und die
weiterhin wirksame Sperre gegen andere Prozesse. Beschädigung, Ablauf, Replay,
Identitätsabweichung, fehlende Dateien, Hardlinks, Symlinks beziehungsweise Reparse
Points und Live-Mutation durch denselben Eigentümer werden ohne echte Zugangsdaten,
App-Konfiguration oder Kundengeräte geprüft.
