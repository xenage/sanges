use super::{BoxCommand, BoxSettingValue, Command, DaemonCommand, HelpTopic, parse, render_help};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn parses_box_list_command() {
    let command = parse(args(&["box", "list"])).expect("parse");

    assert!(matches!(command, Command::Box(BoxCommand::List)));
}

#[test]
fn parses_update_command() {
    let command = parse(args(&["update"])).expect("parse");

    assert!(matches!(command, Command::Update));
}

#[test]
fn parses_daemon_foreground_command() {
    let command = parse(args(&["daemon"])).expect("parse");

    assert!(matches!(command, Command::Daemon(DaemonCommand::Run)));
}

#[test]
fn parses_daemon_log_with_tail_and_follow() {
    let command = parse(args(&["daemon", "log", "--tail", "50", "--follow"])).expect("parse");

    match command {
        Command::Daemon(DaemonCommand::Log(command)) => {
            assert_eq!(command.tail, Some(50));
            assert!(command.follow);
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_help_topic_for_update() {
    let command = parse(args(&["update", "--help"])).expect("parse");

    assert!(matches!(command, Command::Help(HelpTopic::Update)));
}

#[test]
fn parses_help_topic_for_daemon_log() {
    let command = parse(args(&["daemon", "log", "--help"])).expect("parse");

    assert!(matches!(command, Command::Help(HelpTopic::DaemonLog)));
}

#[test]
fn keeps_box_ps_as_alias_for_list() {
    let command = parse(args(&["box", "ps"])).expect("parse");

    assert!(matches!(command, Command::Box(BoxCommand::List)));
}

#[test]
fn parses_box_set_without_box_id() {
    let command = parse(args(&["box", "set", "cpu_cores", "2"])).expect("parse");

    match command {
        Command::Box(BoxCommand::Set(command)) => {
            assert!(command.box_id.is_none());
            assert!(matches!(
                command.value,
                BoxSettingValue::CpuCores { value: 2 }
            ));
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_help_topic_for_box_set() {
    let command = parse(args(&["box", "set", "--help"])).expect("parse");

    assert!(matches!(command, Command::Help(HelpTopic::BoxSet)));
}

#[test]
fn root_help_examples_follow_box_lifecycle() {
    let help = render_help(HelpTopic::Root);

    assert!(help.contains("sagens update"));
    assert!(help.contains("sagens box new"));
    assert!(help.contains("sagens box start <UUID>"));
    assert!(help.contains("sagens box exec <UUID> bash"));
    assert!(!help.contains("sagens box set cpu_cores 2"));
}

#[test]
fn box_list_help_describes_configured_limits() {
    let help = render_help(HelpTopic::BoxList);

    assert!(help.contains("live / configured"));
    assert!(help.contains("Stopped BOXes show `0 / configured`"));
    assert!(help.contains("NET` shows only the BOX network setting"));
}
