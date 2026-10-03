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

Eine Allow-ACE mit der exakten öffentlichen Owner-Rights-SID `S-1-3-4`
repräsentiert den aktuellen Eigentümer ihres Objekts. Sie erhält deshalb eine
objektgebundene Prüfung gegen den Eigentümer aus demselben Security Descriptor.
ACE-SID und Eigentümer müssen gültig und nicht NULL sein. Der Eigentümer muss
weiterhin die unveränderte `trusted_sid`-Regel erfüllen; im geschützten
Konsole-Bereich muss er zusätzlich exakt der aktuellen User-SID entsprechen.
TrustedInstaller als Eigentümer erhält durch diesen Pfad keinen zusätzlichen
Trust außerhalb der Vorfahren-Regel. Owner Rights selbst bleibt als
Objekteigentümer untrusted und ist keine globale Gruppe vertrauenswürdiger SIDs.
Der Creator-Owner-Platzhalter und eine ähnliche SID `S-1-3-5` werden nicht
gleichgesetzt. Jede weitere Allow-ACE bleibt einzeln geprüft und kann weiterhin
das gesamte Objekt blockieren.
Microsoft beschreibt [Owner Rights](https://learn.microsoft.com/en-us/windows-server/identity/ad-ds/manage/understand-security-identifiers)
als Bezug auf den Objekteigentümer, bei dessen ACE die impliziten Rechte
READ_CONTROL und WRITE_DAC ignoriert werden. Die Erkennung verwendet exakt den
[SDK-Typ WinCreatorOwnerRightsSid](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-well_known_sid_type).
Normale ACE-SIDs behalten ihre bisherigen Trustentscheidungen. Eigentümerprüfung,
Mutation-Masken, Reparse-/Hardlink-/Volume-Prüfungen, Handle-Lebensdauer und
Fehlerrückgabe ändern sich nicht.

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

Die Windows-State-Fixture liegt als neues eindeutiges temporäres Verzeichnis
direkt unter dem tatsächlichen `FOLDERID_LocalAppData` des aktuellen
Prozess-Users, nicht im allgemeinen Runner-TEMP oder einem Produktverzeichnis.
Die Python-Probe ermittelt den Elternpfad ausschließlich mit
[`SHGetKnownFolderPath`](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath),
`KF_FLAG_DEFAULT` und NULL-Token. Der SDK-Speicher wird auch bei Fehlern über
`CoTaskMemFree` freigegeben. Es gibt keinen Umgebungsvariablen- oder TEMP-Fallback.
Vor jedem Scratch- oder State-Schreibzugriff verlangt sie einen absoluten
lokalen Diskpfad, `DRIVE_FIXED` und vorhandene Verzeichniskomponenten ohne
Reparse-Attribut. UNC, relative Pfade und API-Fehler blockieren. Die Probe läuft
weiterhin nur mit `GITHUB_ACTIONS=true` unter Windows und gibt ausschließlich
feste Fehlerkategorien aus. Sie ändert weder vorhandene Eigentümer noch ACLs
oder Berechtigungen; Cleanup betrifft nur ihr neu erzeugtes temporäres
Verzeichnis. Der native Gate-Code prüft danach erneut die vollständige
Eigentümer-, ACL- und Pfadhierarchie. Portable Fake-API-Tests prüfen auch den
Abbruch vor Verzeichniserstellung und die Speicherfreigabe im Fehlerfall.

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

Bei `forbidden-access` nennt ausschließlich die Probe zusätzlich die feste
Principal-Klasse der abgewiesenen ACE und einzelne feste Rechtekategorien.
Sie erhält nur `ace.Mask & mutation`, niemals die ungefilterte ACE-Maske.
An Vorfahren sind die möglichen Kategorien `delete`, `delete-child`,
`write-dac`, `write-owner`, `write-attributes`, `write-ea`, `generic-write`
und `generic-all`. Im geschützten Root sowie auf Gate und Journal kommen
`write-data` und `append-data` hinzu. Die Bedeutung der öffentlichen Bits steht
in Microsofts [ACCESS_MASK](https://learn.microsoft.com/en-us/windows/win32/secauthz/access-mask)
und [Dateirechten](https://learn.microsoft.com/en-us/windows/win32/fileio/file-access-rights-constants).
Rohmasken, SIDs, ACL-Inhalte, Pfade und Kontonamen werden nicht ausgegeben.
Ein reiner Rust-Kategorisierungstest prüft alle 1024 Bitkombinationen, die
Nullmaske, Bits außerhalb der Mutation-Masken und beide Hierarchiestufen.
Die Python-Suite kompiliert und führt denselben std-only Helper samt Test auch
ohne Windows-API aus. Die vorhandenen SID-Tests prüfen weiterhin die NULL-SID.
Die Diagnoseausgabe selbst ändert keine Ablehnungsentscheidung.
Mutation-Masken, globale SID-Trust-Prüfung und Rückgabe bleiben gleich; die
objektgebundene Owner-Rights-Auswertung ist oben beschrieben.

Bei abgewiesenem Eigentümer ergänzt ausschließlich die Windows-Probe eine feste
Klasse `system`, `admins`, `builtin-users`, `everyone`, `creator-owner`,
`local-service`, `network-service`, `current-user`, `trusted-installer`,
`all-services`, `authenticated-users`, `owner-rights`, `builtin-guests`,
`builtin-power-users`, `builtin-backup-operators`, `builtin-remote-desktop-users`,
`builtin-remote-management-users`, `windows-account-form` oder `other`.
Dieselben Klassen gelten für die abgewiesene ACE. Dateieigentümer und ACE-SID
sind getrennte Prüfungen. `owner-rights` ist nicht `creator-owner`.
Die sieben zusätzlichen öffentlichen Klassen verwenden ausschließlich
[SDK-Typen](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-well_known_sid_type)
und [IsWellKnownSid](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-iswellknownsid).
Die Hierarchie nennt nur `root-volume`, `protected-root`, `direct-parent` oder
`outer-ancestor`. Die letzten beiden Kategorien folgen ausschließlich der
Position im bereits geprüften Komponentenvektor, ohne Namen oder numerische Tiefe.
Die Klassifikation verwendet exakte
SID-Vergleiche, keine Präfixfreigabe für Service-SIDs. Eine Klassifikation ist
keine Trust-Erteilung. Der native Windows-Test verlangt für beide Produkte,
dass TrustedInstaller ausschließlich an Vorfahren akzeptiert und im geschützten
Root sowie auf Gate und Journal abgewiesen wird. All Services bleibt auf beiden
Hierarchiestufen abgewiesen. Eine synthetische Service-SID, die nur in
der letzten Subautorität von TrustedInstaller abweicht, muss `other` bleiben
und ebenfalls in allen vier Kombinationen abgewiesen werden.

Erst nach den vorhandenen exakten Klassen- und Benutzervergleichen prüft die
Probe die strukturelle Kategorie `windows-account-form`. Sie verlangt eine
gültige, nicht leere SID, NT-Autorität und exakt fünf Subautoritäten. Die einzige
gelesene Subautorität ist der öffentliche erste Wert 21. Vor jedem SDK-Zugriff
prüft sie NULL und `IsValidSid`, vor Index 0 zusätzlich die exakte Anzahl.
Das SDK [validiert den Subautoritätsindex nicht selbst](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsidsubauthority).
Domainanteile und Konto-RID liest, speichert oder protokolliert die Probe nicht.
Die Form unterscheidet weder lokale SAM- von AD-Konten noch Benutzer von Gruppen.
Alle sieben neuen Klassen und eine vollständig synthetische Kontoform bleiben
als eigenständige Principals für beide Produkte an Vorfahren und geschützten
Objekten untrusted. Die Owner-Rights-ACE wird separat an den tatsächlichen
Objekteigentümer gebunden. SDK-Tests
konstruieren diese SIDs tatsächlich und prüfen ähnliche Formen, NULL und Null-SID.
Die Python-Suite führt den originalen std-only Strukturhelper samt Grenztests
und die originale Positionsausdruckslogik auch ohne Windows-API aus.

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
Der [Folgelauf 37107498261](https://github.com/ongrowww/rustdesk-lab/actions/runs/37107498261)
auf Basis `d6a36a25` kompiliert und besteht den erweiterten SID-Klassentest.
Die acht Windows-Lock-Fixtures scheitern mit `forbidden-access` und `ancestor`.
Welcher Principal oder ACE dies verursacht, ist damit nicht belegt. Die neue
Platzierung unter dem OS-Konsole-Elternpfad ist eine Probe-Hypothese, keine
weitere Trust-Ausnahme.
Auch [Lauf 37108565146](https://github.com/ongrowww/rustdesk-lab/actions/runs/37108565146)
auf Basis `bb931d2a` besteht den SID-Test, scheitert aber wieder in allen acht
Windows-Lock-Fixtures mit `forbidden-access` und `ancestor`. Die LocalAppData-
Platzierung hat den Fehler nicht behoben. Die neue ACL-Diagnose soll den
Principal und die abgewiesene Rechteklasse eingrenzen, ohne Trust zu erweitern
oder ACLs zu ändern.
[Lauf 37109618924](https://github.com/ongrowww/rustdesk-lab/actions/runs/37109618924)
auf Basis `1181c958` besteht unter Windows die SDK-SID- und Bitklassentests.
Alle acht Lock-Fixtures scheitern weiterhin an einem Vorfahren. Die abgewiesene
ACE hat Klasse `other` und die maskierten Rechte `delete`, `delete-child`,
`write-dac`, `write-owner`, `write-attributes` und `write-ea`. Die separate
Owner-Prüfung hat zuvor bestanden. Die Ausgabe ist kein Effective-Access-Ergebnis
und begründet keine Trust-Erweiterung. Der macOS-Job besteht.
[Lauf 37111209826](https://github.com/ongrowww/rustdesk-lab/actions/runs/37111209826)
auf Basis `504568b9` besteht die sechs Windows-SDK-/Kategorisierungs- und
Default-off-Tests sowie den macOS-Job. Alle acht Windows-Lock-Fixtures scheitern
weiterhin. Die abgewiesene ACE ist nun als `owner-rights` am `direct-parent`
belegt, nach bereits akzeptiertem Objekteigentümer. Dieser Befund begründet die
enge objektgebundene Auswertung, keinen Gruppen- oder Präfixtrust.
Ein zusätzlicher SDK-Test prüft SYSTEM-/Admin-/TrustedInstaller-/Current-User-
Eigentümer für beide Produkte und Hierarchiestufen, untrusted und NULL-Eigentümer,
vollständig allokierte ausgerichtete Revision-0-SIDs als ungültige ACE/Eigentümer,
Creator Owner und ähnliche SIDs sowie eine separate fremde Allow-ACE neben einer
gültigen Owner-Rights-ACE. Er verändert keine tatsächlichen Host-ACLs.
Der native Windows-Nachweis dieses Fixes und erfolgreiche Windows-Lock-Fixtures
bleiben offen. Das Paket aktiviert weiterhin weder Gate noch App-Updater und
implementiert keinen Installer- oder Staging-Schritt.

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
