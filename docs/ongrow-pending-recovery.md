# Exklusive Übernahme eines bestehenden Pending

`acquire_pending_recovery` liefert crate-intern eine `PendingRecoveryLease`.
Die Build-Time-Policy muss aktiviert sein. Ohne Konfiguration liefert die
Funktion `Disabled`, bevor sie den festen Produkt-Root öffnet.

Die Lease besitzt den vorhandenen exklusiven Kernel-Lock. Die private Factory
prüft an genau diesem Lock, dass er exklusiv ist und sein bestehendes Journal
den einzelnen Pending-Bytewert enthält. Ready wird als `Untrusted` abgelehnt.
Ein belegter Gate-Lock liefert `Busy`. Fehlende oder beschädigte Journale werden
nicht repariert. Die Übernahme schreibt und synchronisiert nichts.

Während der Lease können andere Prozesse weder Sessions noch Installation oder
Recovery übernehmen. Drop und Prozessabsturz geben nur den Kernel-Lock frei.
Pending bleibt bestehen und ein weiterer Recovery-Eigentümer kann übernehmen.

Diese Lease erlaubt nur die spätere Untersuchung des offenen Versuchs. Sie
ist kein Installer-Ticket und bestätigt weder Releaseidentität noch Gesundheit.
Sie hat keine Clone-, Path-, Commit-, Ready-, Rollback- oder Cleanup-Methode.
Der Gesamtupdater ist damit noch nicht fertig.

## Isolierter Nachweis

`python3 scripts/test_ongrow_pending_recovery.py` prüft die unveränderten
Originalbytes und bisherigen Additionsblöcke, kompiliert das Originalmodul
mit Rust 1.81 und führt neun native Sachtests aus. Ein zusätzlicher Test ist
der eigene Kindprozess-Einstieg. Echte Kindprozesse belegen Shared-, Pending-
und Recovery-Locks. Die Probe prüft Übernahme nach kill/wait, feste Dateiidentität,
unveränderte Bytes und Shared-plus-Pending-Ablehnung durch dieselbe Factory.
Kindprozess-Guards beenden und warten ausschließlich ihre eigenen Prozesse.
Ein weiterer nativer Lauf setzt `RUST_TEST_THREADS=1`, damit der serielle
libtest-Zeilenpräfix den begrenzten Kindprozess-Handshake nicht verhindert.

Das neue Compile-Time-Merkmal `ongrow_pending_recovery_probe` ist nur in der
separaten Probe gesetzt. Ohne Merkmal und Root entdeckt die gewöhnliche
App-Modulform keine nativen Recovery-Tests. Fehlender Probeinput muss fehlschlagen.
macOS verwendet eigene 0700-Verzeichnisse unter dem ignorierten `target`.
Windows prüft den bestehenden Known-Folder-Probeparent vor allen Seiteneffekten.
Manifeste und Pins stammen aus den bisherigen OS-Fixtures. Cargo.lock wird nur
in den temporären Build kopiert. Lokale Builds verwenden den vorhandenen Cache
offline. Die CI führt kleine native Probes auf macOS und Windows aus, keine App.

## Offene Aktivierung

Für eine produktive Recovery fehlen insbesondere ein geschützter,
authentifizierter Attempt-Datensatz, separate Windows-MSI- und macOS-Sparkle-
Autorität, die Guardian-Lebensdauer und eine authentifizierte Health-Prüfung
der tatsächlichen neuen App oder des Services. Der geschützte Sequenzcommit
muss vor Ready erfolgen. Recovery-Discard und der echte Desk-SYSTEM-
Cross-Principal-Vertrag brauchen eigene Pakete. Developer-ID und Notarisierung
bleiben ein separater Produktionsschritt. Pending allein ist keine
authentifizierte Releaseidentität.
