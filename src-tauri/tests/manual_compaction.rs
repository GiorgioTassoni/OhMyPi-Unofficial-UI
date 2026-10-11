//! Exercise the real host and pump against a deterministic RPC peer, without
//! provider credentials or changing any user conversation.
mod support;

use std::path::PathBuf;
use std::time::Duration;

use omp_desktop::{models, session};
use omp_transport::{ClientOptions, SidecarSpec};
use support::{registry, RecordingSink};

const PEER: &str = r#"
const readline = require('node:readline');
let tokens = 33000;
const output = value => process.stdout.write(JSON.stringify(value) + '\n');
output({type:'ready', protocolVersion:1, supportedProtocolVersions:[1], maxFrameBytes:1048576, maxReassembledFrameBytes:67108864});
readline.createInterface({input:process.stdin}).on('line', async line => {
  const command = JSON.parse(line);
  let data = {};
  if (command.type === 'get_state') data = {
    sessionId:'compaction-test', sessionName:'Compaction test', isCompacting:false,
    contextUsage:{tokens, contextWindow:100000, percent:tokens / 1000}
  };
  if (command.type === 'compact') {
    await new Promise(resolve => setTimeout(resolve, 350));
    if (command.customInstructions === 'fail') {
      output({type:'response', id:command.id, command:command.type, success:false, error:'Summary generation failed'});
      return;
    }
    tokens = 11000;
    data = {summary:'A compacted summary', tokensBefore:33000};
  }
  output({type:'response', id:command.id, command:command.type, success:true, data});
});
"#;

#[tokio::test]
async fn manual_compaction_publishes_progress_and_refreshes_usage_without_engine_events() {
    let sink = RecordingSink::default();
    let live = session::open(
        &SidecarSpec {
            program: PathBuf::from("bun"),
            args: vec!["-e".into(), PEER.into()],
            cwd: None,
            env: vec![],
        },
        ClientOptions {
            request_timeout: Duration::from_millis(200),
            ..Default::default()
        },
        sink.clone(),
        registry(),
    )
    .await
    .expect("the RPC peer opens");

    for (instructions, succeeds) in [(None, true), (Some("fail"), false)] {
        let call = models::compact(&live, instructions);
        tokio::pin!(call);
        tokio::select! {
            result = &mut call => panic!("compaction settled before progress could be observed: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(150)) => {}
        }
        assert!(live.status().unwrap().control.is_compacting);
        assert!(sink.patches().iter().any(|patch| patch
            .rows
            .iter()
            .any(|row| row.custom_type.as_deref() == Some("manual_compaction_start"))));
        assert_eq!(call.await.is_ok(), succeeds);
        assert!(!live.status().unwrap().control.is_compacting);
        assert_eq!(
            live.status().unwrap().control.context.unwrap().percent,
            11.0
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let expected_role = if succeeds {
                    "notice:info"
                } else {
                    "notice:error"
                };
                if sink.patches().iter().any(|patch| {
                    patch.rows.iter().any(|row| {
                        row.custom_type.as_deref() == Some("manual_compaction_end")
                            && row.role == expected_role
                    })
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the receipt is published without another engine event");
    }
    live.shutdown(Duration::from_secs(1)).await;
}
