# Vertrag für signierte OnGROW-Updateinformationen

Stand des Codes: Offline-Verifier, lokaler Lab-Release-Produzent und gesperrte
Upstream-Updatewege. Kein Autoupdater, Downloader oder Installer ist angebunden. Es gibt keinen
produktiven Release-Endpunkt und keinen eingebauten Release-Public-Key.
Eine erfolgreich geprüfte Information ist nur ein `VerifiedCandidate`.

## Produkt und Manifest

Schema 1 akzeptiert ausschließlich `customer-desk` und `support-console`, die
Plattform `windows-x64` und die Kanäle `lab` oder `stable`. Ein lokaler Aufrufer
legt Produkt, Plattform und Kanal fest. Ein Manifest muss genau dazu passen.
Andere Plattformen brauchen einen späteren Vertragsentscheid.

Das Manifest ist ein UTF-8-JSON-Objekt mit höchstens 65.536 Bytes. Alle folgenden
Felder sind erforderlich. Unbekannte oder doppelte Felder sind ungültig, auch
bei unterschiedlich escaped geschriebenen JSON-Schlüsseln.

| Feld | Typ und Bedeutung |
| --- | --- |
| `schema_version` | Integer, genau `1` |
| `product` | `customer-desk` oder `support-console` |
| `platform` | `windows-x64` |
| `channel` | `lab` oder `stable` |
| `release_sequence` | Unsigned 64-bit Integer, monotoner OnGROW-Releasezähler |
| `upstream_version` | Nichtleerer ASCII-Versionstext, höchstens 64 Bytes, nur Buchstaben, Ziffern, Punkt, Bindestrich und Unterstrich |
| `source_sha` | Git-Quellrevision, genau 40 kleine Hex-Zeichen |
| `issued_at` | Unsigned 64-bit Integer, Unix-Sekunden |
| `expires_at` | Unsigned 64-bit Integer, Unix-Sekunden |
| `filename` | ASCII-Basename, höchstens 255 Bytes, nur Buchstaben, Ziffern, Punkt, Bindestrich und Unterstrich |
| `size` | Unsigned 64-bit Integer, positive Größe der vollständigen Setup-Datei in Bytes |
| `sha256` | SHA-256 der vollständigen Setup-Datei, genau 64 kleine Hex-Zeichen |
| `download_url` | Absolute kanonische HTTPS-URL innerhalb der lokalen Allowlist |

`filename` erlaubt keine Verzeichnisse, Windows-ADS, Laufwerksangaben, `.` oder
`..`, abschließenden Punkt oder Windows-Gerätenamen. `CON`, `PRN`, `AUX`, `NUL`,
`COM1` bis `COM9` und `LPT1` bis `LPT9` sind auch mit Erweiterung verboten.
Die konservative ASCII-Regel schließt Unicode-Gerätenamen und Leerzeichen aus.

## Signatur und Payload

Der Signierer verwendet Ed25519 detached. Die zu signierende Nachricht ist die
folgende Bytefolge, ohne JSON-Kanonisierung und ohne vorgeschalteten Hash:

```text
ASCII("OnGROW signed update manifest v1") || 0x00 || exakte Manifestbytes
```

Die externe Signatur hat genau 64 Bytes. Der lokal vertrauenswürdige Public-Key
hat genau 32 Bytes. Beide API-Eingaben sind rohe Bytes, keine Base64-Texte. Der
Public-Key kommt nie aus dem Manifest. Der Verifier authentifiziert die Bytes
vor dem JSON-Parsing. Jede Byteänderung macht die Signatur ungültig, auch bloße
Whitespace-Änderungen. Es gibt keinen unsignierten Fallback.

`VerifiedCandidate::verify_payload` prüft die Länge und berechnet SHA-256 über
alle tatsächlich übergebenen Setup-Bytes. Dateiname und Größe allein genügen
nicht. Der Verifier liest keine Dateien und erzeugt keinen Shellbefehl. Eine
spätere Integration muss dieselben geprüften Bytes bis zur Installation gegen
Änderungen schützen. Die heutige API behauptet diesen Schutz nicht.

Die Kryptoprüfung verwendet die vorhandene libsodium-Bindung. Sie entspricht
dem [detached Ed25519-Verfahren von libsodium](https://doc.libsodium.org/public-key_cryptography/public-key_signatures).
Eine signierte Manifestdatei ist keine Authenticode-Signatur von Setup.exe und
keine notarisierten macOS-App. Diese Produktprüfungen bleiben getrennt.

## Zeit, Reihenfolge und lokaler Zustand

Zeit ist eine explizite Eingabe, kein Systemclock-Zugriff des Verifiers.
`issued_at` darf nicht nach dem lokalen Prüfzeitpunkt liegen. `expires_at`
muss nach dem Prüfzeitpunkt und nach `issued_at` liegen. Eine spätere Integration
muss eine vertrauenswürdige Zeitquelle und den Umgang mit Uhrfehlern festlegen.

`release_sequence` muss sowohl den fest in der aktuellen Release hinterlegten
Zähler als auch die explizit übergebene zuletzt akzeptierte Sequenz übersteigen.
Mehrere OnGROW-Releases dürfen dieselbe RustDesk-Upstream-Version verwenden.
Der Upstream-Versionstext ist deshalb kein Anti-Downgrade-Zähler.

Die API akzeptiert `LastAcceptedSequence::Known` oder `Unknown`. Bei unbekanntem
oder beschädigtem Zustand lehnt sie die Information ab. Einen Zahlenwert aus
einer benutzermanipulierbaren Config darf ein späterer Aufrufer nicht als
geschützten Zustand ausgeben. Beim vertrauenswürdigen Erstbootstrap muss eine
bekannte Anfangssequenz festgelegt werden. Es gibt hier keine Persistenz und
keinen Replay-Schutz über Prozessneustarts hinweg.

Ein bewusstes Recovery zu älteren Payload-Inhalten braucht ein neu ausdrücklich
signiertes Release mit höherer OnGROW-Sequenz. Ein allgemeiner Downgrade-Schalter
oder das Zurücksetzen des Sequenzzustands ist nicht vorgesehen.

## Download-Adressvertrag

`DownloadLocation::new` bekommt einen lokal vertrauenswürdigen HTTPS-Origin und
einen absoluten, mit Slash abgeschlossenen Pfadprefix. Dieses Modul legt keinen
OnGROW-Serverpfad fest. Die URL muss exakt diesen Origin und Prefix verwenden;
der letzte Pfadbestandteil muss dem Manifest-Dateinamen entsprechen.

Credentials, Query, Fragment, Backslash, Prozentencodings, Unicode-Zeichen,
leere Pfadbestandteile und Punktsegmente sind verboten. Der Verifier prüft den
rohen Text vor der URL-Normalisierung und verlangt anschließend die kanonische
Schreibweise. Er akzeptiert daher weder encoded Slashes oder Traversal noch
normalisierte `:443`-Varianten, Host-Lookalikes oder nur ähnlich benannte Prefixe.
Ein späterer Downloader muss Redirects gesondert gegen die Allowlist prüfen;
der Offline-Verifier führt keine HTTP-Anfrage aus.

## Upstream-Sperre und Produktidentität

Die beiden OnGROW-Produkte verwenden bis zur vollständigen Integration keinen
RustDesk-Updatefallback. Ein `OnceLock` friert den Entscheid aus dem gebackenen
`hbb_common::APP_NAME` vor dem Laden von Custom-Client-Branding ein. Die Namen
sind `OnGROW Support Desk` und `OnGROW Support Console`. Ein optionales
Compile-Profil `ONGROW_PRODUCT_ROLE` sperrt die beiden Rollen zusätzlich.
Der Console-Labworkflow setzt diese Rolle. Der unveränderte Deskworkflow nutzt
das gebackene Produktprofil. Spätere Umbenennung hebt die Sperre nicht auf.

`core_main` initialisiert diese Regel vor `global_init` und lehnt einen echten
`--update`-Befehl vor Bootstrap oder Elevation ab. Die Guard-Funktionen prüfen
außerdem Softwarecheck, manuelle und automatische Scheduler, Downloadpfad,
privilegierten Apply, Windows-Staging, macOS-Update-Lock, DMG-Verarbeitung und
updatebezogenes Cleanup. Die Flutter-Aktionen `download-new-version`,
`update-me` und `extract-update-dmg` erreichen den gesperrten Downloadpfad.
Der Legacy-Updateaufruf ist ebenfalls gesperrt. Normales `--install`,
`--silent-install`, `install_me` und `goto_install` bleiben unverändert.

Diese Identitätsregel setzt das korrekt angewendete Buildprofil voraus. Der
Verifier ist auch ohne Flutter kompilierbar; er ersetzt keinen Schutz gegen
manipulierte ausführbare Dateien oder einen kompromittierten Buildprozess.
Plain RustDesk behält seine bisherigen Updatewege und URL-Validierung.

## Schlüsselbetrieb und spätere Aktivierung

Der Release-Signierschlüssel bleibt offline und getrennt von hbbs-, Geräte-
oder Admin-Schlüsseln. Identitäten dürfen nicht wiederverwendet werden. Ein
vertrauenswürdiger Client-Public-Key-Bootstrap bleibt deaktiviert, bis seine
getrennte Einrichtung ausdrücklich freigegeben ist. Der lokale Produzent kann
ein ausdrücklich autorisiertes Lab-Keypaar außerhalb von Git neu anlegen.
Tests erzeugen eigene synthetische Keypairs ausschließlich zur Laufzeit,
geben keine privaten Werte aus und verwenden niemals das dauerhafte Lab-Keypaar.

## Lokaler Lab-Release-Produzent

`scripts/ongrow_release_signing.py` verwendet ausschließlich die Python-
Standardbibliothek und ein ausdrücklich ausgewähltes vorhandenes OpenSSL 3.
Es installiert keine Werkzeuge, lädt nichts hoch und verändert keine
Client-Konfiguration. Ed25519 verwendet `pkeyutl -sign -rawin`, ohne Digest oder
Prehash, gemäß der [OpenSSL-Dokumentation](https://docs.openssl.org/3.0/man1/openssl-pkeyutl/).
Der private PEM-Inhalt fließt bei der Erzeugung direkt von OpenSSL in einen
exklusiv angelegten Dateideskriptor. Python liest ihn nicht. Diagnostik von
OpenSSL-Schlüsseloperationen wird abgefangen und nicht weitergereicht.

Alle Pfade müssen absolute, nicht umgeleitete lokale Pfade sein. Auf macOS
sind temporäre Pfade zuvor zu kanonisieren, damit der Systemalias `/var`
nicht als Eingabepfad verwendet wird. Bestehende Schlüssel- oder Releaseziele,
Symlinks, Reparse Points, Hardlinks, unerwartete Eigentümer und unsichere
Rechte werden abgelehnt. Der Produzent repariert keine vorhandenen Rechte.
Schlüsseldateien benötigen unter POSIX den aktuellen Benutzer als Eigentümer,
Modus `0600` und ein unmittelbares Elternverzeichnis mit `0700`.
Andere Eltern dürfen dem aktuellen Benutzer oder root gehören, dürfen aber
nicht gruppen- oder weltbeschreibbar sein. Die lokalen CLI-Tests legen ihre
privaten temporären Fixtureverzeichnisse außerhalb von Git im Benutzerverzeichnis an.

Nur für das ausdrücklich gewählte Homebrew-Werkzeug
`/opt/homebrew/opt/openssl@3/bin/openssl` und dessen kanonischen
`/opt/homebrew/Cellar/openssl@3/<version>/bin/openssl` wird der konkrete
Cellar-Elternpfad mit `0775` akzeptiert, wenn er nicht umgeleitet ist, dem
aktuellen Benutzer gehört und seine tatsächliche Gruppe `admin` ist.
Alle übrigen Werkzeugeltern und die reguläre ausführbare Werkzeugdatei
bleiben strikt geprüft. Diese eng begrenzte Werkzeugausnahme gilt niemals für
Schlüssel, Payload oder Output. Das ausgewählte lokale Werkzeug und die
Administratoren werden vertraut; der Signierer schützt nicht vor einer
administrativ kompromittierten Toolchain.

Für ein neues, ausdrücklich freigegebenes externes Verzeichnis:

```sh
OPENSSL_TOOL=/absolute/path/to/trusted/openssl3
KEY_DIR=/absolute/private/path/to/new-lab-key-directory
python3 scripts/ongrow_release_signing.py init-key \
  --directory "$KEY_DIR" --openssl "$OPENSSL_TOOL"
```

`init-key` legt ausschließlich neue Verzeichnisse mit `0700` an und schreibt
`windows-lab-ed25519.pem` mit `0600` sowie
`windows-lab-ed25519.pub` als rohe 32 öffentliche Bytes. Das Ed25519-SPKI
aus OpenSSL wird vor dem Ableiten dieser Bytes exakt geprüft. Als Ergebnis
erscheinen nur Erfolg und der SHA-256-Fingerprint des öffentlichen Schlüssels.
Ein vorhandenes Ziel wird nicht wiederverwendet oder überschrieben.

Erst mit einem tatsächlich gebauten und geprüften Eingangspaket, nach grüner
nativer Verifier-CI und gesonderter Auslieferungsfreigabe, ist ein echter
Release zulässig. Der Signierer kann EXE, ZIP und MSI als Payload signieren.
Die MSI-Erweiterung erlaubt den vorhandenen WiX/MSI-Weg als späteren Produzenten,
baut oder installiert aber selbst kein MSI. Keine Dateiendung beweist
Produktidentität, Installerqualität, Lizenzvollständigkeit oder Authenticode.
Diese Prüfungen und die Build-Provenienz bleiben Aufgabe des Eingangspakets.

Die folgenden Variablen müssen aus dem geprüften Paket und der freigegebenen
Releaseplanung stammen. Beispiel-Origin und Prefix sind keine produktive
Downloadkonfiguration. Der Output bezeichnet ein neues Releaseverzeichnis;
sein sicherer Elternpfad muss bereits existieren.

```sh
python3 scripts/ongrow_release_signing.py sign \
  --payload "$VERIFIED_PAYLOAD" \
  --key-file "$KEY_DIR/windows-lab-ed25519.pem" \
  --public-key-file "$KEY_DIR/windows-lab-ed25519.pub" \
  --openssl "$OPENSSL_TOOL" --output-dir "$NEW_RELEASE_DIR" \
  --product customer-desk --platform windows-x64 --channel lab \
  --release-sequence "$NEXT_SEQUENCE" --previous-sequence "$PREVIOUS_SEQUENCE" \
  --version "$UPSTREAM_VERSION" --source-sha "$VERIFIED_SOURCE_SHA" \
  --issued-at "$ISSUED_UNIX_SECONDS" --expires-at "$EXPIRES_UNIX_SECONDS" \
  --origin https://updates.example.test --path-prefix /releases/
```

Für Console-Pakete wird ausdrücklich `--product support-console` angegeben.
Der Produzent erlaubt nur `lab` und `windows-x64`, keine Stable-Releases.
Sequenzen und Zeiten sind u64-Werte; der neue Zähler muss die ausdrücklich
angegebene vorherige Sequenz übersteigen. Der Zeitraum muss beim Signieren
gültig sein. `previous-sequence` ist eine Produzenteneingabe und kein
geschützter persistenter Clientzustand.

Der sichere Payload-Basename darf nicht mit den Metadatendateien kollidieren.
Die URL entsteht aus kanonischem HTTPS-DNS-Origin, validiertem Prefix,
Release-Sequenz und Basename. Ein freier Download-URI wird nicht übernommen.
Quelle und Output dürfen nicht überlappen; Schlüssel müssen außerhalb von
Git und außerhalb des Output liegen. Das Paket wird exklusiv kopiert;
Quelländerungen während des Kopierens führen zum Abbruch. Größe und SHA-256
werden über die tatsächlich kopierten Bytes berechnet.

Der Output enthält genau die unveränderte Payload, `manifest.sig` mit 64 Bytes
und deterministisches Schema-1-UTF-8-JSON als `manifest.json`. Die Signatur
authentifiziert Domain inklusive Nullbyte und exakt diese JSON-Bytes.
OpenSSL prüft sie gegen den expliziten lokalen Public-Key nach.
`manifest.json` entsteht erst nach erfolgreicher Prüfung. Fehlercleanup
entfernt nur nachweislich eigene unveränderte Inodes, nicht fremde Restdateien.
Es gibt keinen `latest`-Rewrite, keinen eingebetteten Public-Key als
Client-Bootstrap und keine Auto-Apply-Freigabe. Manifests bleiben öffentliche
Releaseinformationen, keine Schlüssel- oder Gerätedatenspeicher.

Vor Produktionsaktivierung sind Kundenclient-Abnahme, vertrauenswürdiger
Key-Bootstrap, Schlüsselrotation, Kompromittierungsbehandlung und ein Recovery-
Verfahren erforderlich. Dieser begrenzte Verifier ist kein TUF-Client. Die
Rotations- und Kompromittierungsanforderungen müssen vor Aktivierung gegen ein
bestehendes Framework wie [TUF](https://theupdateframework.io/docs/overview/)
geprüft werden.

Ein Folgepaket braucht Downloadlimits, geschütztes Staging, Schutz vor Änderungen
zwischen Prüfung und Nutzung, Signatur- und Plattformprüfung, Session-Lock,
Installationsidentität, geschützte Sequenzpersistenz und Recovery-Tests. Es darf
den Kandidaten nicht einfach an die vorhandenen Upstream-Installer durchreichen.

## Prüfungen und Grenzen

Die Modul-Unittests verwenden echte libsodium-Signaturen für gültige und
manipulierte Daten, fremde Schlüssel, falsche Bytegrößen, JSON-Fehler,
Produkt-/Kanalabweichungen, unbekannte Architektur, Ablauf, Zukunftsdatum,
Replay/Downgrade, Payload-Länge/-Hash und URL-/Dateinamenangriffe. Eigene
`OnceLock`-Instanzen testen, dass spätere Brandänderungen die Sperre nicht lösen.
Quelltextprüfungen kontrollieren den ersten Guard an den Update-Einstiegen und
die frühe CLI-Reihenfolge. Sie sind kein ausgeführter Plattform-Lifecycle-Test.

Der vorbereitete native Windows-Console-Labworkflow führt nach Erzeugung der
Bridge-Dateien aus. Im Linux-Bridgejob testet er den Produzenten mit echtem,
vorhandenem OpenSSL 3 und exportiert ausschließlich vier öffentliche
synthetische Dateien unter dem ignorierten `target/ongrow-release-test-fixtures`:
`public.key`, `manifest.json`, `manifest.sig` und `payload.bin`.
Nur diese vier zusätzlichen Dateien werden im Bridge-Artefakt transportiert,
niemals der private synthetische Schlüssel. Der Windowsjob setzt
`ONGROW_RELEASE_TEST_FIXTURE_DIR` im Rust-Teststep und braucht kein OpenSSL.

```sh
cargo test --locked --lib --features flutter ongrow_update
```

Der Interoperabilitätstest ruft die unveränderten echten `verify_manifest`-
und `verify_payload`-Funktionen mit dem OpenSSL-Export auf und prüft gültige
Bytes sowie Manifest- und Payload-Manipulationen. Ohne Fixturevariable meldet
ein lokaler Lauf ausdrücklich, dass diese Interoperabilitätsprüfung nicht
ausgeführt wurde. In CI ist eine fehlende Variable ein harter Testfehler.
Das ist kein Nachweis einer ausgeführten Windows-Installation oder Auto-Apply.

Ein isolierter lokaler Harness kann dieselben unveränderten Moduldateien mit
den vorhandenen Lockfile-Versionen von serde, sha2, url und sodiumoxide testen,
ohne Desktop-Build. Er beweist die Kryptoprüfung, nicht die Kompilierbarkeit der
gesamten Windows-/macOS-Anwendung. Grüne native CI bleibt Freigabevoraussetzung.
