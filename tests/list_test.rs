#![allow(dead_code)]

use cloud139::commands::list;

#[test]
fn test_list_args_defaults() {
    let args = list::ListArgs {
        path: "/".to_string(),
        page: 1,
        page_size: 100,
        output: None,
    };
    assert_eq!(args.path, "/");
    assert_eq!(args.page, 1);
    assert_eq!(args.page_size, 100);
    assert_eq!(args.output, None);
}

#[test]
fn test_list_args_with_output() {
    let args = list::ListArgs {
        path: "/test".to_string(),
        page: 2,
        page_size: 50,
        output: Some("/tmp/output.json".to_string()),
    };
    assert_eq!(args.output, Some("/tmp/output.json".to_string()));
}

#[test]
fn test_list_args_custom_page() {
    let args = list::ListArgs {
        path: "/".to_string(),
        page: 5,
        page_size: 20,
        output: None,
    };
    assert_eq!(args.page, 5);
    assert_eq!(args.page_size, 20);
}

#[test]
fn test_format_size_zero() {
    assert_eq!(list::format_size(0), "0 B");
}

#[test]
fn test_format_size_very_large() {
    assert_eq!(list::format_size(10737418240), "10.00 GB");
    assert_eq!(list::format_size(107374182400), "100.00 GB");
}

#[test]
fn test_parse_personal_time_edge_cases() {
    assert_eq!(list::parse_personal_time(""), "");
    assert!(list::parse_personal_time("2024-01-01T10:00:00Z").contains("2024"));
    assert!(list::parse_personal_time("2024-01-01T10:00:00.000").contains("2024"));
    assert!(list::parse_personal_time("2024-01-01 10:00:00").contains("2024"));
}
