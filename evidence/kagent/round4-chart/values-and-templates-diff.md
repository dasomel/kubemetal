# Values and template evidence

[measured — see kagent_1_0_0_alpha17_show_values.stdout.txt] `database.postgres.ownerConnectionStringSecretRef` defaults to name `kagent-postgres-owner`, key `ownerConnectionString`; `readWriteConnectionStringSecretRef` defaults to name `kagent-postgres-readwrite`, key `readWriteConnectionString`. The latter has a `Required.` comment.

[measured — see kagent_1_0_0_alpha17_template_default.rc.txt and kagent_1_0_0_alpha17_template_custom_values.rc.txt] Both saved template return codes are 0.

[measured — see kagent_1_0_0_alpha17_template_default.stderr.txt and kagent_1_0_0_alpha17_template_custom_values.stderr.txt] Neither saved stderr contains a template requirement error; both record OCI pull metadata.

[pending probe] Re-capture values, Chart.yaml metadata and rendered output from the exact hashed local archives. The previous claims about template guards, ignored keys and complete chart removals are withdrawn; rendered output alone does not reveal a template's conditional source.

[unverified] Values comments and rendered references do not prove database connectivity, workload startup, runtime compatibility or successful installation.
