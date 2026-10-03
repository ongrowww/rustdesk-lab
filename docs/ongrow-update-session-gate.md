# Updateinstallation und Sitzungssperre

Der Gate-Code ist standardmäßig aus. Dieses Paket aktiviert weder Downloads noch
Installationen und ersetzt nicht den weiterhin gesperrten Upstream-Updater.
Erst ein Build mit `ONGROW_UPDATE_SESSION_GATE=v1` und einer bekannten gebackenen
Produktrolle verlangt die vorbereitete Sitzungssperre. Die Laufzeitumgebung,
GUI-Einstellungen und Serverantworten können diese Entscheidung nicht ändern.
Unbekannte explizite Gate-Werte oder Rollen werden abgewiesen.

## Feste geschützte Dateien

Beide Dateien liegen außerhalb des austauschbaren App-Bundles beziehungsweise
MSI-Payloads. Ein Update darf sie weder löschen noch durch neue Inodes ersetzen.

| Produkt | macOS | Windows |
| --- | --- | --- |
| Kundenclient | `/Library/Application Support/OnGROW/Support Desk Update` | OS-Known-Folder ProgramData, dann `OnGROW/Support Desk Update` |
| Konsole | UID-basierter Account-Home, dann `Library/Application Support/OnGROW/Support Console Update` | OS-Known-Folder LocalAppData des Prozesstokens, dann `OnGROW/Support Console Update` |

Der UID-Home wird mit `getpwuid_r` ermittelt. Windows verwendet
`SHGetKnownFolderPath`, nicht HOME, APPDATA oder PROGRAMDATA. Andere GUI-Pfade und
IPC-Pfade sind keine Gate-Eingaben.

- `admission-v1.lock` ist ein leeres reguläres File mit genau einem Hardlink.
- `state-v1.journal` ist genau ein Byte lang. `0` bedeutet Ready, `1` Pending.
- Alle anderen Werte, Längen, fehlenden Dateien und Lesefehler blockieren neue
  Sitzungen. Es gibt keinen Zeitablauf und kein PID-basiertes Entsperren.

Auf macOS müssen Kundenclient-Dateien und sämtliche Anker root gehören. Für die
Konsole sind root oder die tatsächliche Prozess-UID als Vorfahren zulässig, der
geschützte Konsole-Root und die Dateien gehören genau dieser UID. Gruppen- und
Fremdschreibrechte werden abgewiesen. Komponenten werden mit `openat`,
`O_NOFOLLOW` und Handle-Metadaten geprüft, inklusive lokaler Dateisysteme.
Extended Allow-ACLs mit Mutationsrechten werden konservativ vollständig
abgewiesen, auch wenn sie für den Eigentümer gelten.

Apple libc liefert für ein vorhandenes File ohne Extended-ACL NULL/ENOENT.
Nur diese unmittelbar erfasste Kombination gilt nach erneuter vollständiger
Prüfung desselben Handles als fehlende ACL. Vorhandene ACLs werden mit
`acl_valid` geprüft und nach Darwin-Semantik vollständig durchlaufen.
Alle anderen ACL-Fehler blockieren.

Unter Windows gelten SYSTEM und die lokale Administratorengruppe als
vertrauenswürdige Desk-Eigentümer und schreibberechtigte Principals. Die aktuelle
User-SID ist für Desk-Anker ausdrücklich nicht ausreichend. Bei der Konsole
kommt genau diese SID hinzu; Konsole-Root und Dateien müssen ihr gehören.
Nur an Vorfahren gilt zusätzlich die exakte öffentliche TrustedInstaller-SID
als vertrauenswürdig. Sie erhält keinen Trust im geschützten Root, auf Gate
oder Journal. All Services und andere Service-SIDs bleiben überall abgewiesen.
Handles prüfen Eigentümer, DACL, Reparse-Attribute und Hardlink-Anzahl. Vorfahren
bleiben ohne FILE_SHARE_DELETE geöffnet. UNC, Netzwerk-/Wechseldatenträger und
Reparse-Komponenten werden abgewiesen. Fremde Rechte zur Erstellung von
Geschwistern an Vorfahren sind zulässig, nicht Rechte zum Löschen oder
Rekonfigurieren bestehender geschützter Kinder. Im geschützten Root und auf den
Dateien werden sämtliche fremden Schreibrechte abgewiesen.

## Lebensdauer einer Sitzung

Eine eingehende Verbindung nimmt vor Handshake und Wake-up eine nicht blockierende
Shared-Sperre. `Connection::start` hält sie bis zum tatsächlichen Verbindungsende.
Ausgehend erfolgt zusätzlich eine frühe Prüfung vor Schlüsselbeschaffung und
RDP-/Portforward-Verzweigung. Der zentrale `Client::start` gibt eine eigene Lease
mit dem Stream zurück.

Der normale I/O-Loop hält diese Lease bis zum Ende. `connect_and_login` gibt sie
mit dem Stream zurück, und der separat gestartete `run_forward` hält sie bis zu
seinem Ende. Eine geschlossene UI oder ein beendeter Listener lässt laufende
Forward-Tasks deshalb nicht ungeschützt zurück. Ein nur lokal wartender
Portforward-Listener hält keine dauerhafte Sitzungslease. Die frühe Lease wird
vor diesem Listener freigegeben, jeder neue Remote-Forward muss erneut durch
die zentrale Admission. Lease-Clones teilen ein Kernel-Handle; erst der letzte
Drop gibt die Shared-Sperre frei.

macOS verwendet `flock`, Windows `LockFileEx` auf dem festen Bereich ab Offset 0,
Länge 1. Beide Zugriffe sind nicht blockierend. Busy bedeutet später erneut
versuchen, nicht Sitzungen beenden oder auf Mutex-/Prozesslisten ausweichen.

## Installations- und Crashzustand

Der Installer erhält die Exclusive-Sperre erst nach allen Shared-Leases. Bevor
er irgendeinen App- oder Serviceprozess stoppt, schreibt er Pending unter dieser
Sperre in das feste Ein-Byte-Journal und wartet auf den vollständigen Flush.
macOS verlangt `sync_all` und `F_FULLFSYNC`, Windows `FlushFileBuffers` über
`File::sync_all`. Der Journal-Inode wird nicht ausgetauscht. Leser können während
des Schreibens keine Sitzung beginnen.

Ein Installer-Drop oder Absturz löscht Pending nicht. Auch ein neuer Installer
darf ein bestehendes Pending nicht als frische Installation übernehmen. Erst
der noch zu implementierende authentifizierte Health-/Recovery-Pfad darf mit
Exclusive-Sperre Ready schreiben. `VerifiedHealth` besitzt bewusst noch keinen
Produktionskonstruktor. Die Test-Fixture erzeugt ihre eigene Test-Capability.

`initialize` ist eine getrennte Bootstrap-API. Der privilegierte Bootstrap muss
zuerst den festen Root und dessen geschützte Eigentümer/ACLs erstellen. Danach
legt die API beide Dateien ausschließlich neu an. Bereits eine einzige
vorhandene Datei verhindert Initialisierung, auch bei unvollständigem Zustand.
Keine Session legt Dateien an oder repariert Zustände.

## Tests und offene Nachweise

`python scripts/test_ongrow_update_session_gate.py` kompiliert ausschließlich den
echten Gate-Code in einem temporären eigenen Cargo-Workspace. Auf lokalen Macs
läuft das mit dem vorhandenen gepinnten libc offline. Die Windows-Probe läuft
ausschließlich auf dem separaten Windows-Runner. Dessen temporäre Lockdatei
übernimmt die bestehenden gepinnten Abhängigkeitsversionen, Root-Cargo-Dateien
bleiben unverändert. Weder RustDesk noch Dienst, Netzwerk oder Kundenregistrierung
werden gestartet.

Nur dieser isolierte Compileraufruf setzt `--cfg ongrow_session_gate_probe`.
Die Fixture verweigert ohne diesen Marker das Kompilieren. Normale App-Tests
entdecken ausschließlich den Default-off-Policy-Test, nicht die nativen
Fixtures. Ein zusätzlicher schmaler Import derselben Quelldatei unter
`ongrow_update::session_gate` prüft diese Trennung ohne Desktop-Build und ohne
Testroot. Innerhalb der nativen Probe führt ein fehlender Testroot dagegen
ausdrücklich zum Testfehler. Der Child-Testname wird aus dem echten Modulpfad
ermittelt, nicht aus einem fest angenommenen App-Modulnamen.

Die Windows-Probe gibt bei Trust-Ablehnungen nur feste Kategorien aus, etwa
`owner-trust`, `forbidden-access` oder `directory-open`, und den festen Kontext
`ancestor`, `protected-root`, `journal-bootstrap` oder `gate-bootstrap`.
Pfade, SIDs, ACL-Inhalte und Kontonamen werden nicht ausgegeben. Diese Ausgabe
existiert ausschließlich bei `cfg(test)` zusammen mit dem Probe-Marker.
Die Diagnose selbst ändert keine Trust-Regel.

Bei abgewiesenem Eigentümer ergänzt ausschließlich die Windows-Probe eine feste
Klasse `system`, `admins`, `builtin-users`, `everyone`, `creator-owner`,
`local-service`, `network-service`, `current-user`, `trusted-installer`,
`all-services` oder `other`. Die Hierarchie nennt zusätzlich `root-volume`, niemals einen
Pfad oder dessen konkrete Komponenten. Die Klassifikation verwendet exakte
SID-Vergleiche, keine Präfixfreigabe für Service-SIDs. Eine Klassifikation ist
keine Trust-Erteilung. Der native Windows-Test verlangt für beide Produkte,
dass TrustedInstaller ausschließlich an Vorfahren akzeptiert und im geschützten
Root sowie auf Gate und Journal abgewiesen wird. All Services bleibt auf beiden
Hierarchiestufen abgewiesen. Eine synthetische Service-SID, die nur in
der letzten Subautorität von TrustedInstaller abweicht, muss `other` bleiben
und ebenfalls in allen vier Kombinationen abgewiesen werden.

Produktion und Probe teilen dieselbe Konstruktion der exakten öffentlichen
TrustedInstaller-SID
`S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464`. Die
All-Services-SID `S-1-5-80-0` bleibt ausschließlich in der Probe. Beide verwenden
das bereits verfügbare `ConvertStringSidToSidW`, prüfen die SID mit `IsValidSid`
und geben zugewiesenen Speicher über `AllocatedSid` und `LocalFree` frei. Es gibt
keine Konto- oder Domainabfrage und keinen Netzwerk-Fallback. Eine fehlgeschlagene
TrustedInstaller-Konstruktion erteilt keinen Trust; die Diagnose liefert `other`.
Microsoft veröffentlicht die TrustedInstaller-SID im Abschnitt Machine
Path/Folder der
[WindowsAppSDK-ApplicationData-Spezifikation](https://github.com/microsoft/WindowsAppSDK/blob/main/specs/applicationdata/ApplicationData.md#341-machine-pathfolder).
Die öffentlichen SID-Kategorien sind in
[Microsoft Security identifiers](https://learn.microsoft.com/en-us/windows-server/identity/ad-ds/manage/understand-security-identifiers)
beschrieben.

Der [native Diagnoselauf 37106638612](https://github.com/ongrowww/rustdesk-lab/actions/runs/37106638612)
auf Basis `af58da41` kompiliert und besteht den SID-Klassentest unter Windows.
Alle acht Lock-Fixtures scheitern dagegen mit `trusted-installer`, `owner-trust`
und `root-volume`. Der macOS-Job besteht. Dieser Beleg begründet die begrenzte
Vorfahren-Regel, keinen pauschalen Trust für Eltern oder Service-SIDs.
Der native Windows-Nachweis für die korrigierte Regel ist noch offen.

Die lokalen macOS-Regressionsfälle prüfen unter anderem Mehrprozess-Locks,
natürlichen Drop und harten Prozessabbruch, dauerhaftes Pending, letzte
Lease-Clone/Subtask-Lebensdauer, unveränderte Gate-/Journal-Inodes, ungültige und
fehlende Zustände, unlesbares Journal, Hardlinks, Symlinks sowie schreibende ACLs
auf Root, Gate und Journal. Der Gesamt-Prozessaufruf hat 180 Sekunden Timeout.
Der Child-Readiness-Handshake besitzt noch keinen eigenen kürzeren Watchdog.

Folgende Nachweise sind ausdrücklich offen und werden nicht durch bestandene
Quellprüfungen oder einen grünen allgemeinen Probe-Lauf ersetzt:

- Tatsächlicher Windows-Kompilier-/Kernel-Lock-Nachweis auf dem Windows-Runner.
- Cross-UID-/Cross-SID-Ersetzungsversuche und tatsächliche fremde Eigentümer an
  Journal/Gate sowie root-/SYSTEM-eigene Desk-Fixtures auf isolierten Runnern.
- Windows-DACL- und Unreadable-Journal-Negativfälle unter verschiedenen Principals.
- Vollständiger App-Build und echte Remote-/Reconnect-/RDP-/Forward-Integration
  beider Produkte und Plattformen mit aktiviertem Gate.
- Privilegierter Bootstrap, authentisierte Installer-IPC, geschütztes Staging,
  erneute Verifikation im privilegierten Prozess und Health-/Recovery-Commit.
- Wiederanlauf und Health-Rollback nach erfolgreichem MSI-Commit oder App-Swap.

Vor diesen Nachweisen bleibt das Gate ausgeschaltet. Eine MSI-Transaktion allein
belegt keine App-Gesundheit nach dem Commit.
