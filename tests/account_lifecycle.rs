use minimalmonitor::{
    auth::{ABSOLUTE_MS, Auth, IDLE_MS},
    bounds::WorkBounds,
    error::Error,
    ownership::StateOwner,
    storage::{
        Storage,
        security::{Operation, Value},
    },
};
use zeroize::Zeroizing;

#[tokio::test]
async fn password_login_persistence_reset_and_all_session_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let bounds = WorkBounds::default();
    let auth = Auth::new(storage.client(), bounds.clone()).await.unwrap();
    let secret = || Zeroizing::new("controlled fixture passphrase".to_owned());
    auth.local_account(" Admin ".into(), secret(), false)
        .await
        .unwrap();
    assert!(matches!(
        auth.local_account("other".into(), secret(), false).await,
        Err(Error::Conflict)
    ));
    let signed = auth.preauth(1000).unwrap();
    auth.check_preauth(&signed, &signed, 1001).unwrap();
    assert_eq!(
        auth.check_preauth(&signed, &signed, 1000 + minimalmonitor::auth::PREAUTH_MS),
        Err(Error::Forbidden)
    );
    assert_eq!(
        auth.check_preauth(&signed, &signed, 999),
        Err(Error::Forbidden)
    );
    let mut forged = signed.clone().into_bytes();
    forged[20] = if forged[20] == b'a' { b'b' } else { b'a' };
    let forged = String::from_utf8(forged).unwrap();
    assert_eq!(
        auth.check_preauth(&forged, &forged, 1001),
        Err(Error::Forbidden)
    );
    let session = auth
        .login("ADMIN".into(), secret(), "127.0.0.1".parse().unwrap())
        .await
        .unwrap();
    assert_ne!(session.token, signed);
    assert_eq!(
        minimalmonitor::auth::hex(&auth.session(&session.token).await.unwrap()),
        session.csrf
    );
    let retained = session.token.clone();
    let db = rusqlite::Connection::open(root.join("monitor.db")).unwrap();
    let stored: Vec<u8> = db
        .query_row("SELECT token_hash FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, minimalmonitor::auth::digest(retained.as_bytes()));
    assert_ne!(stored, retained.as_bytes());
    drop(db);
    drop(auth);
    storage.shutdown().await.unwrap();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let auth = Auth::new(storage.client(), bounds.clone()).await.unwrap();
    assert!(auth.session(&retained).await.is_ok());
    let hold = bounds.password().unwrap();
    assert!(matches!(
        auth.login("admin".into(), secret(), "127.0.0.2".parse().unwrap())
            .await,
        Err(Error::Busy)
    ));
    drop(hold);
    assert_eq!(
        auth.change(
            &retained,
            Zeroizing::new("wrong current passphrase".into()),
            Zeroizing::new("changed controlled passphrase".into())
        )
        .await,
        Err(Error::Unauthorized)
    );
    assert!(auth.session(&retained).await.is_ok());
    // A busy login and the rejected password change consumed the two initial
    // global attempts. Verify the limiter, then use a fresh test instance for
    // the independent successful-change behavior without wall-clock sleeps.
    assert_eq!(
        auth.change(
            &retained,
            secret(),
            Zeroizing::new("changed controlled passphrase".into())
        )
        .await,
        Err(Error::RateLimited)
    );
    let change_auth = Auth::new(storage.client(), bounds.clone()).await.unwrap();
    change_auth
        .change(
            &retained,
            secret(),
            Zeroizing::new("changed controlled passphrase".into()),
        )
        .await
        .unwrap();
    assert!(matches!(
        auth.session(&retained).await,
        Err(Error::Unauthorized)
    ));
    auth.local_account("admin".into(), secret(), true)
        .await
        .unwrap();
    let fresh = Auth::new(storage.client(), bounds).await.unwrap();
    assert!(fresh.check_preauth(&signed, &signed, 1001).is_err()); // previous signing key cannot replay after restart
    assert!(matches!(
        fresh
            .login("missing".into(), secret(), "127.0.0.1".parse().unwrap())
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        fresh
            .login(
                "admin".into(),
                Zeroizing::new("incorrect fixture passphrase".into()),
                "127.0.0.1".parse().unwrap()
            )
            .await,
        Err(Error::Unauthorized)
    ));
    storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn deterministic_expiry_oldest_eviction_generation_races_and_logout() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::start(StateOwner::acquire(&dir.path().join("state")).unwrap())
        .await
        .unwrap();
    let c = storage.client();
    // Storage fixtures never go through a verifier; no real credentials.
    c.security(Operation::Local {
        user: "fixture".into(),
        hash: "noncredential-fixture".into(),
        reset: false,
        now: 1000,
    })
    .await
    .unwrap();
    for i in 1..=6 {
        c.security(Operation::Login {
            generation: 1,
            token: [i; 32],
            csrf: [100 + i; 32],
            now: 1000 + i as i64,
        })
        .await
        .unwrap();
    }
    assert!(matches!(
        c.security(Operation::Session {
            token: [1; 32],
            now: 1007
        })
        .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        c.security(Operation::Session {
            token: [2; 32],
            now: 1007
        })
        .await,
        Ok(Value::Session(_))
    ));
    assert!(matches!(
        c.security(Operation::Session {
            token: [3; 32],
            now: 1003 + IDLE_MS
        })
        .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        c.security(Operation::Session {
            token: [4; 32],
            now: 1003
        })
        .await,
        Err(Error::Unauthorized)
    ));
    // Keep a session active up to the absolute boundary without extending it.
    for offset in 1..=12 {
        let now = 1006 + offset * (IDLE_MS - 1);
        if now < 1006 + ABSOLUTE_MS {
            c.security(Operation::Session {
                token: [6; 32],
                now,
            })
            .await
            .unwrap();
        }
    }
    assert!(matches!(
        c.security(Operation::Session {
            token: [6; 32],
            now: 1006 + ABSOLUTE_MS
        })
        .await,
        Err(Error::Unauthorized)
    ));
    c.security(Operation::Login {
        generation: 1,
        token: [7; 32],
        csrf: [107; 32],
        now: 1000 + ABSOLUTE_MS,
    })
    .await
    .unwrap();
    c.security(Operation::Logout {
        token: [7; 32],
        now: 1001 + ABSOLUTE_MS,
    })
    .await
    .unwrap();
    assert!(matches!(
        c.security(Operation::Session {
            token: [7; 32],
            now: 1002 + ABSOLUTE_MS
        })
        .await,
        Err(Error::Unauthorized)
    ));
    c.security(Operation::Login {
        generation: 1,
        token: [8; 32],
        csrf: [108; 32],
        now: 1003 + ABSOLUTE_MS,
    })
    .await
    .unwrap();
    c.security(Operation::Local {
        user: "fixture".into(),
        hash: "replacement-fixture".into(),
        reset: true,
        now: 1004 + ABSOLUTE_MS,
    })
    .await
    .unwrap();
    assert!(matches!(
        c.security(Operation::Login {
            generation: 1,
            token: [9; 32],
            csrf: [109; 32],
            now: 1005 + ABSOLUTE_MS
        })
        .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        c.security(Operation::Change {
            token: [8; 32],
            generation: 1,
            hash: "stale-fixture".into(),
            now: 1005 + ABSOLUTE_MS
        })
        .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        c.security(Operation::Session {
            token: [8; 32],
            now: 1005 + ABSOLUTE_MS
        })
        .await,
        Err(Error::Unauthorized)
    ));
    storage.shutdown().await.unwrap();
}
