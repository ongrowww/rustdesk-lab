//! Runtime isolation for CI and app-launch smoke tests.
//! Never compile this switch into the delivered product profile.

pub fn networking_disabled() -> bool {
    isolation_requested(
        std::env::var("ONGROW_CI_SMOKE_TEST").ok().as_deref(),
        std::env::var("GITHUB_ACTIONS").ok().as_deref(),
    )
}

fn isolation_requested(smoke: Option<&str>, github_actions: Option<&str>) -> bool {
    matches!(smoke, Some("1") | Some("true")) || matches!(github_actions, Some("1") | Some("true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_customer_process_keeps_networking() {
        assert!(!isolation_requested(None, None));
        assert!(!isolation_requested(Some("0"), Some("false")));
    }

    #[test]
    fn smoke_process_cannot_use_live_servers() {
        assert!(isolation_requested(Some("1"), None));
        assert!(isolation_requested(Some("true"), Some("false")));
    }

    #[test]
    fn github_runner_is_isolated_even_when_smoke_flag_is_missing_or_disabled() {
        assert!(isolation_requested(None, Some("true")));
        assert!(isolation_requested(Some("0"), Some("true")));
    }

    #[test]
    #[ignore = "invoked in a child process with a controlled environment"]
    fn environment_probe() {
        let expected = std::env::var("ONGROW_ISOLATION_EXPECTED").unwrap();
        assert_eq!(networking_disabled(), expected == "true");
    }

    #[test]
    fn real_process_environment_is_checked_at_runtime() {
        for (smoke, github, expected) in [
            (None, None, false),
            (Some("1"), None, true),
            (None, Some("true"), true),
            (Some("0"), Some("true"), true),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            let (_, module) = module_path!().split_once("::").unwrap();
            let probe = format!("{}::environment_probe", module);
            child.args(["--ignored", "--exact", &probe]);
            child
                .env_remove("ONGROW_CI_SMOKE_TEST")
                .env_remove("GITHUB_ACTIONS");
            child.env("ONGROW_ISOLATION_EXPECTED", expected.to_string());
            if let Some(value) = smoke {
                child.env("ONGROW_CI_SMOKE_TEST", value);
            }
            if let Some(value) = github {
                child.env("GITHUB_ACTIONS", value);
            }
            let output = child.output().unwrap();
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }
}
