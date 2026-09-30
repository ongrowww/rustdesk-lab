# Vertrag für signierte OnGROW-Updateinformationen

Stand des Codes: reiner Offline-Verifier und gesperrte Upstream-Updatewege.
Kein Autoupdater, Downloader oder Installer ist angebunden. Es gibt keinen
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
vertrauenswürdiger Public-Key-Bootstrap bleibt deaktiviert, bis seine getrennte,
opake Einrichtung ausdrücklich freigegeben ist. Dieses Paket erzeugt oder
speichert keine dauerhaften Schlüssel. Tests erzeugen synthetische Keypairs
ausschließlich zur Laufzeit und geben deren Werte nicht aus.

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
Bridge-Dateien aus:

```sh
cargo test --locked --lib --features flutter ongrow_update
```

Ein isolierter lokaler Harness kann dieselben unveränderten Moduldateien mit
den vorhandenen Lockfile-Versionen von serde, sha2, url und sodiumoxide testen,
ohne Desktop-Build. Er beweist die Kryptoprüfung, nicht die Kompilierbarkeit der
gesamten Windows-/macOS-Anwendung. Grüne native CI bleibt Freigabevoraussetzung.
