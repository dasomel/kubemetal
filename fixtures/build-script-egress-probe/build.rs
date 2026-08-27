// 이슈 #36 네거티브 픽스처: 이 build.rs는 "egress가 차단되어야 빌드가 통과하는"
// 크레이트다 — 일반적인 build.rs와 반대 방향의 성공/실패 판정을 갖는다.
//
// - example.com:443 연결에 성공하면 → egress가 차단되지 않았다는 뜻이므로
//   build.rs가 실패한다(exit 1).
// - 연결이 타임아웃/거부/DNS 실패로 안 되면 → egress가 차단됐다는 뜻이므로
//   build.rs가 성공한다(exit 0).
//
// `.github/workflows/supply-chain-fixture.yml`이 harden-runner의
// `egress-policy: block`으로 이 크레이트를 빌드해 실제로 outbound 네트워크가
// 막혀 있는지를 증명한다. 로컬(비차단 환경)에서 `cargo build`를 돌리면
// example.com에 도달 가능하므로 의도대로 **실패**하는 것이 정상이다.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn main() {
    let target = "example.com:443";

    let addrs = match target.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<_>>(),
        Err(e) => {
            // DNS 조회 자체가 막히는 경우도 "차단 성공"으로 처리한다.
            println!("cargo:warning=egress 차단 확인됨 (DNS 조회 실패: {e})");
            return;
        }
    };

    if addrs.is_empty() {
        println!("cargo:warning=egress 차단 확인됨 (DNS 조회 결과 없음)");
        return;
    }

    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            Ok(_) => {
                panic!(
                    "egress blocked 안 됨 — example.com({addr}) 도달 가능. \
                     harden-runner block 정책이 적용되지 않았거나 allowed-endpoints에 \
                     example.com이 포함되어 있습니다."
                );
            }
            Err(e) => {
                println!("cargo:warning=egress 차단 확인됨 ({addr} 연결 실패: {e})");
            }
        }
    }
}
