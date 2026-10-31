//! #17: end-to-end redaction proof. Plants sentinel secrets in every input the bundle collects,
//! then greps the CONTENT of every produced file (the verifier only checks redaction metadata).
use super::*;

struct TestDir(PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// (class, sentinel) - 각 sentinel은 유일하고 grep 가능해야 실패 시 누출 클래스가 드러난다.
const SENTINELS: &[(&str, &str)] = &[
    ("bearer", "SENT-BEARER-7f3a91c2"),
    ("authorization-basic", "SENT-AUTHBASIC-2b8d40e6"),
    ("authorization-shell-quoted", "SENT-AUTHQUOTED-6a1c93d2"),
    ("authorization-shell-continued", "SENT-AUTHCONT-4e7b2a91"),
    ("authorization-split-quote", "SENT-AUTHSPLIT-1d58c3f0"),
    ("authorization-unclosed-json", "SENT-AUTHUNCLOSED-8b3f1e57"),
    ("kubeconfig-client-key-data", "SENT-CLIENTKEY-c41e7a09"),
    ("kubeconfig-token", "SENT-KCTOKEN-5d92b3f8"),
    ("pem-private-key", "SENT-PEMBODY-a60c18d7"),
    ("url-userinfo-password", "SENT-URLPASS-9e1f4b52"),
    ("url-userinfo-user", "SENT-URLUSER-3c7a0d84"),
    ("api-key-sk", "sk-SENTAPIKEY0123456789abcdef"),
    ("health-json-secret", "SENT-HEALTHSECRET-e8b25f10"),
];

/// 어떤 규칙에도 해당하지 않는 음성 대조군: 번들에서 반드시 발견되어야 한다.
const NEGATIVE_CONTROL: &str = "NEGCTRL-uncovered-b7d3e915";

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn s(class: &str) -> &'static str {
    SENTINELS.iter().find(|(c, _)| *c == class).unwrap().1
}

/// 센티넬을 심고 번들을 생성해 (번들 파일 경로, 전체 내용) 목록을 돌려준다.
fn bundle_contents() -> (TestDir, Vec<(PathBuf, String)>) {
    // Unix 소켓 경로 길이 때문에 /tmp 기반 (verification_tests.rs와 동일 관례)
    let temp = TestDir(PathBuf::from("/tmp").join(format!(
        "sre-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    let src = temp.0.join("src");
    fs::create_dir_all(&src).unwrap();

    let app_log = src.join("app.log");
    fs::write(
        &app_log,
        format!(
            "2026-01-01 request Authorization: Bearer {bearer}\n\
             Authorization: Basic {hdr}\n\
             curl -H 'Authorization: Bearer-less {quoted}' https://x\n\
             curl -H 'Authorization: Basic\n  {cont}' https://x\n\
             curl -H \"Authorization:\" \"Basic {split}\" https://x\n\
             {{\"cmd\":\"curl -H 'Authorization: Basic {unclosed}\",\"n\":1}}\n\
             clone https://deploy:{urlpass}@git.example.com/repo.git\n\
             clone https://{urluser}:pw@git.example.com/repo2.git\n\
             key = {apikey}\n\
             note {neg}\n",
            bearer = s("bearer"),
            hdr = s("authorization-basic"),
            quoted = s("authorization-shell-quoted"),
            cont = s("authorization-shell-continued"),
            split = s("authorization-split-quote"),
            unclosed = s("authorization-unclosed-json"),
            urlpass = s("url-userinfo-password"),
            urluser = s("url-userinfo-user"),
            apikey = s("api-key-sk"),
            neg = NEGATIVE_CONTROL,
        ),
    )
    .unwrap();

    let kube_log = src.join("kube.log");
    fs::write(
        &kube_log,
        format!(
            "users:\n- name: admin\n  user:\n    client-key-data: {ckd}\n    token: {kct}\n\
             -----BEGIN PRIVATE KEY-----\nMIIEvQIBADAN{pem}\nline2\n-----END PRIVATE KEY-----\n\
             after-pem {neg}\n",
            ckd = s("kubeconfig-client-key-data"),
            kct = s("kubeconfig-token"),
            pem = s("pem-private-key"),
            neg = NEGATIVE_CONTROL,
        ),
    )
    .unwrap();

    let health = serde_json::json!({
        "status": "ok",
        "client_secret": s("health-json-secret"),
        "note": NEGATIVE_CONTROL,
    })
    .to_string();

    let result = generate_support_bundle(
        &temp.0.join("out"),
        None,
        Some(health),
        collect_app_os_version_info(None),
        vec![
            ("app.log".to_string(), app_log),
            ("logs/kube.log".to_string(), kube_log),
        ],
    )
    .unwrap();

    let mut files = Vec::new();
    walk(Path::new(&result.bundle_dir), &mut files);
    let contents = files
        .into_iter()
        .map(|p| {
            let text = String::from_utf8_lossy(&fs::read(&p).unwrap()).into_owned();
            (p, text)
        })
        .collect();
    (temp, contents)
}

fn leaked(contents: &[(PathBuf, String)], class: &str) -> Vec<String> {
    let sentinel = s(class);
    contents
        .iter()
        .filter(|(_, text)| text.contains(sentinel))
        .map(|(p, _)| p.display().to_string())
        .collect()
}

#[test]
fn bundle_files_contain_no_planted_secret_classes() {
    let (_temp, contents) = bundle_contents();
    // 음성 대조군: 걷기/읽기 자체가 동작함을 증명 (규칙 밖 문자열은 남아 있어야 함).
    // 심은 세 출처 각각에서 발견되어야 한다 - 출처 하나가 조용히 빠지면 실패한다.
    for source in ["app.log", "kube.log", "health_summary.json"] {
        assert!(
            contents
                .iter()
                .any(|(p, t)| p.ends_with(source) && t.contains(NEGATIVE_CONTROL)),
            "negative control missing from {source}: that source was dropped or not read"
        );
    }
    assert!(
        contents.iter().any(|(p, _)| p.ends_with(MANIFEST_FILE)),
        "manifest not walked"
    );
    let failures: Vec<String> = SENTINELS
        .iter()
        .filter_map(|(class, _)| {
            let hits = leaked(&contents, class);
            (!hits.is_empty()).then(|| format!("{class} leaked in {hits:?}"))
        })
        .collect();
    assert!(failures.is_empty(), "secret leak: {failures:#?}");
    // A JSON log line carrying an unclosed shell quote must stay parseable after redaction.
    let app = contents
        .iter()
        .find(|(p, _)| p.ends_with("app.log"))
        .map(|(_, t)| t)
        .unwrap();
    let json_line = app.lines().find(|l| l.starts_with("{\"cmd\"")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(json_line).unwrap();
    assert_eq!(parsed["n"], 1);
}
