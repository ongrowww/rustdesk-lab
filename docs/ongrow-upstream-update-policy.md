# Upstream-Aktualisierungen für OnGROW Support Desk

## Ziel

OnGROW übernimmt nur veröffentlichte stabile RustDesk-Versionen. Ein neuer Commit auf
`rustdesk/rustdesk:master` löst weder einen Produkt-Build noch einen Merge oder eine
Veröffentlichung aus. Das Produktprofil, der hbbs-Fork und beide Protobuf-Baselines
werden als gemeinsame, getestete Version behandelt.

## Eingangskontrolle

1. Die offiziellen Client- und Server-Releases werden auf neue stabile Tags geprüft.
   Ein optionaler zeitgesteuerter Workflow darf ausschließlich einen Bericht oder ein
   Issue erzeugen.
2. Für einen Kandidaten wird ein kurzlebiger Intake-Branch vom aktuell ausgelieferten
   OnGROW-Commit angelegt. Upstream wird zuerst mit `git fetch upstream --tags`
   aktualisiert. Verglichen werden Tag, Commit, Changelog und Security-Hinweise.
3. Die kleine OnGROW-Patchserie wird einzeln auf den neuen stabilen Tag übertragen.
   Konflikte werden nicht automatisch aufgelöst. Das Produktprofil muss weiterhin an
   genau eindeutigen Quellankern ansetzen und bei Drift abbrechen.
4. Die Gitlinks für `libs/hbb_common` werden explizit protokolliert. Protobuf-Feldnummern,
   Signaturkontexte und der kanonische Attestierungsvektor werden vor einem Merge mit
   dem Server-Fork verglichen. Eine Feldkollision ist ein Abbruchgrund.

## Regressionsmatrix

Vor einem Merge müssen mindestens folgende Kombinationen grün sein:

| Bereich | macOS ARM64 | Windows x64 |
| --- | --- | --- |
| Frische Installation ohne Alt-Konfiguration | Pflicht | Pflicht, sobald Plan 010 umgesetzt ist |
| OnGROW-Rendezvous und hbbs-Trust-Anchor | Pflicht | Pflicht |
| Explizite kundenseitige Serverüberschreibung | Pflicht | Pflicht |
| Geräteanmeldung, Custom-ID und Attestierung | Pflicht | Pflicht |
| Unbeaufsichtigten Zugriff aktivieren und widerrufen | Pflicht | Pflicht |
| Signiertes, notarisiertes Produktionsartefakt | Plan 008 | Plan 008 |

Zusätzlich muss das Cross-Repository-Kompatibilitätsgate des Server-Forks mit den
festgehaltenen Client-, Server- und beiden `hbb_common`-Revisionen erfolgreich sein.
Ein Produkt-Merge erfolgt erst nach diesem Gate und den plattformspezifischen Tests.

## Vertrauensanker und Rotation

Rendezvous-Hostname, öffentlicher hbbs-Trust-Anchor und HTTPS-Control-Plane-URL sind
nicht geheime Repository-Variablen. Fehlende oder ungültige Werte brechen den Build vor
der Installation teurer Toolchains ab. Private Server- oder Signaturschlüssel dürfen
niemals in Client-Builds, Repository-Variablen, Artefaktprovenienz oder Logs gelangen.

Eine hbbs-Schlüsselrotation benötigt einen koordinierten Client-Release. Alter und neuer
Trust-Anchor dürfen nur über eine ausdrücklich geprüfte Übergangsstrategie parallel
betrieben werden. Ein stilles Austauschen des Schlüssels ohne Client-Rollout ist nicht
zulässig.

## Rollback

Für jeden Produkt-Release werden Quell-Commit, alle Submodule, Patch-Digest,
Trust-Anchor-Fingerprint und Artefaktprüfsummen festgehalten. Bei einer Regression wird
auf das letzte vollständig geprüfte Set aus Client, Server und Konfiguration
zurückgerollt. Datenbank- oder Protokolländerungen müssen vorab eine separat geprüfte
Rückwärtsstrategie besitzen. Ein Rollback darf keine explizite Kundenkonfiguration
löschen oder überschreiben.

## Merge- und Freigaberegel

Der Intake-Branch wird erst nach Code-Review, Kompatibilitätsgate und kompletter
Regressionsmatrix in den OnGROW-Feature-Branch übernommen. Build, Signierung,
Notarisierung, Veröffentlichung und Deployment bleiben getrennte, ausdrücklich
freizugebende Schritte. Automatische Übernahmen aus `master` sind ausgeschlossen.
