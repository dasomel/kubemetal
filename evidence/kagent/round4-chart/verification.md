# Offline reproduction and verification (#193)

Run from the repository root. These Python one-liners read committed host files only; no probe, Helm, network, installs or cluster are needed. Each was run before writing the corresponding rows. The first command reproduces chart metadata/hash rows (including absent appVersion), then inventory, full-schema counts, each kind’s selected field/type rows and literal candidates, template argv/rc/stderr/hash records, and values comments.

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); [(print(f.name, [(x.split(':',1)[0],x.split(':',1)[1].strip()) for x in f.read_text().splitlines() if x.startswith(('name:','version:','appVersion:'))], 'appVersion present:',any(x.startswith('appVersion:') for x in f.read_text().splitlines()))) for f in sorted(p.glob('*-show-chart.stdout.txt'))]; [(print(f.name,f.read_text().strip())) for f in sorted(p.glob('*-sha256.stdout.txt'))]"
```

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); [(print(f.name,json.loads(f.read_text()))) for f in sorted(p.glob('*-inventory.json'))]"
```

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); d=json.loads((p/'schema-diff.json').read_text()); [print(k,{n:len(v) for n,v in x.items()}) for k,x in d.items() if k.startswith('kagent.dev/v1alpha2/')]"
```

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); a=json.loads((p/'0-kagent-crds-0.9.12-paths.json').read_text())['kagent.dev/v1alpha2/Agent']; b=json.loads((p/'1-kagent-crds-1.0.0-alpha17-paths.json').read_text())['api.kagent.dev/v1alpha3/Agent']; [print(x,a.get(x),b.get(x)) for x in ['spec.type', 'spec.description', 'spec.declarative', 'spec.declarative.runtime', 'spec.declarative.modelConfig', 'spec.declarative.systemMessage', 'spec.declarative.deployment', 'spec.declarative.deployment.resources', 'spec.declarative.deployment.resources.limits', 'spec.declarative.deployment.resources.limits.*', 'spec.declarative.deployment.resources.requests', 'spec.declarative.deployment.resources.requests.*', 'spec.declarative.tools', 'spec.declarative.tools[]', 'spec.declarative.tools[].type', 'spec.declarative.tools[].mcpServer', 'spec.declarative.tools[].mcpServer.apiGroup', 'spec.declarative.tools[].mcpServer.kind', 'spec.declarative.tools[].mcpServer.name', 'spec.declarative.tools[].mcpServer.toolNames', 'spec.declarative.tools[].mcpServer.toolNames[]']]; [print('candidate',x,b.get(x)) for x in ['spec.template.inline.description', 'spec.template.inline.modelConfig.name', 'spec.template.inline.systemPrompt', 'spec.template.inline.tools', 'spec.template.inline.tools[]', 'spec.template.inline.tools[].mcp', 'spec.template.inline.tools[].mcp.server.apiGroup', 'spec.template.inline.tools[].mcp.server.kind', 'spec.template.inline.tools[].mcp.server.name', 'spec.template.inline.tools[].mcp.tools', 'spec.template.inline.tools[].mcp.tools[]']]"
```

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); a=json.loads((p/'0-kagent-crds-0.9.12-paths.json').read_text())['kagent.dev/v1alpha2/ModelConfig']; b=json.loads((p/'1-kagent-crds-1.0.0-alpha17-paths.json').read_text())['api.kagent.dev/v1alpha3/ModelConfig']; [print(x,a.get(x),b.get(x)) for x in ['spec.model', 'spec.provider', 'spec.openAI', 'spec.openAI.baseUrl', 'spec.apiKeySecret', 'spec.apiKeySecretKey']]; [print('candidate',x,b.get(x)) for x in []]"
```

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); a=json.loads((p/'0-kagent-crds-0.9.12-paths.json').read_text())['kagent.dev/v1alpha2/RemoteMCPServer']; b=json.loads((p/'1-kagent-crds-1.0.0-alpha17-paths.json').read_text())['api.kagent.dev/v1alpha3/RemoteMCPServer']; [print(x,a.get(x),b.get(x)) for x in ['spec.description', 'spec.protocol', 'spec.url', 'spec.sseReadTimeout', 'spec.terminateOnClose', 'spec.timeout']]; [print('candidate',x,b.get(x)) for x in []]"
```

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); c=json.loads((p/'commands.json').read_text()); print('result',json.loads((p/'result.json').read_text())); [print(x['name'],x['argv'],x['exit_code'],x['stdout']['truncated'],x['stderr']['truncated'],repr((p/x['stderr']['file']).read_text())) for x in c if 'template' in x['name']]; print((p/'template-outputs.sha256.txt').read_text())"
```

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); f=p/'1-kagent-1.0.0-alpha17-show-values.stdout.txt'; lines=f.read_text().splitlines(); [print(i+1,s) for i,s in enumerate(lines) if any(t in s for t in ['ownerConnectionString','readWriteConnectionString','Required.,kagent-postgres-'])]"
```

[unverified] These checks reproduce recorded artifact facts, not installation, startup, admission/CEL, conversion webhooks, migration or runtime behavior. ModelConfig Helm-values-to-CR mapping and semantic equivalence of candidate paths remain unverified.

Common root types, metadata boundary and emitted resource-map alternatives:

```sh
python3 -B -c "import json,pathlib; p=pathlib.Path('evidence/kagent/round4-chart/host-probe'); [(print(f.name,k,{x:v.get(x) for x in ['apiVersion','kind','metadata','spec']},{x:y for x,y in v.items() if x.startswith(('metadata.','spec.declarative.deployment.resources.limits.*','spec.declarative.deployment.resources.requests.*'))})) for f in sorted(p.glob('*-paths.json')) for k,v in json.loads(f.read_text()).items() if '/v1alpha1/' not in k and any(k.endswith('/'+t) for t in ['Agent','ModelConfig','RemoteMCPServer'])]"
```

[measured — local docs-fill verification] `python3 -B -m unittest discover -s tests/kagent -v`: `Ran 6 tests in 0.242s`, `OK` (exit 0). Synthetic fixture/unit coverage only. `git diff --check`: no output (exit 0).

[measured — repository CLAUDE.md] The Claude adapter additionally references runtime skill loading and Claude-only orchestration rules; this lane changes documentation only. Application code, Helm values, probe tooling, raw host evidence and cluster state were left unchanged.
