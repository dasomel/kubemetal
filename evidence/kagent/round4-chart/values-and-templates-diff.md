# Values and template evidence

| Chart | version | appVersion | tgz SHA256 | Evidence (`host-probe/`) |
|---|---|---|---|---|
| kagent | 0.9.12 | absent | `a6aede4fd39aab4ea0292b68a59ed2b0e41afc50f1e8de8a9a2ead2a89bd5075` | [measured — host-probe/0-kagent-0.9.12-show-chart.stdout.txt; host-probe/0-kagent-0.9.12-sha256.stdout.txt] |
| kagent-crds | 0.9.12 | absent | `977e59c4ad7f751f2badea53d5f698220f37a1af28fb282fd7917103a084e7f3` | [measured — host-probe/0-kagent-crds-0.9.12-show-chart.stdout.txt; host-probe/0-kagent-crds-0.9.12-sha256.stdout.txt] |
| kagent | 1.0.0-alpha17 | absent | `9260dad01ef544c684ca7d749e31b46443100192c318281c4fdffe8e79d86543` | [measured — host-probe/1-kagent-1.0.0-alpha17-show-chart.stdout.txt; host-probe/1-kagent-1.0.0-alpha17-sha256.stdout.txt] |
| kagent-crds | 1.0.0-alpha17 | absent | `8c236ee9cd3730a2275814d048fccf51a35d197ff5e8f624623bdf4cc415c431` | [measured — host-probe/1-kagent-crds-1.0.0-alpha17-show-chart.stdout.txt; host-probe/1-kagent-crds-1.0.0-alpha17-sha256.stdout.txt] |

[measured — host-probe/1-kagent-1.0.0-alpha17-show-values.stdout.txt] `database.postgres.ownerConnectionStringSecretRef` defaults to name `kagent-postgres-owner`, key `ownerConnectionString`; `readWriteConnectionStringSecretRef` defaults to name `kagent-postgres-readwrite`, key `readWriteConnectionString`. The latter comment says `Required.`; this is a values comment, not a measured template requirement.

[measured — host-probe/commands.json] All four recorded default `helm template km-r4 <local-tgz> --namespace kagent --include-crds` commands returned 0, without stream truncation; each recorded stderr file is empty. No template error establishing required-at-template-time values was recorded. This run did not pass the app's custom values.

[measured — host-probe/template-outputs.sha256.txt] Template stdout files were removed for size; only their SHA256 records remain. No rendered template content is cited or inferred here.

[unverified] Required values under other inputs, ignored values, template conditional behavior, installation, DB connectivity, startup, admission, conversion webhooks, migration and runtime behavior.
