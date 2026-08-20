//! 도메인 모델 불변식 및 기본 동작 단위 테스트
//! Unit tests for domain model invariants and basic behaviors

use calljet::model::{
    CallKind, Confidence, Language, SourceLocation, SourceRange, SymbolId, SymbolQuery,
};

#[test]
fn test_language_display() {
    assert_eq!(Language::C.to_string(), "C");
    assert_eq!(Language::Cpp.to_string(), "C++");
}

#[test]
fn test_source_location_and_range() {
    let loc1 = SourceLocation::new("src/main.cpp", 10, 5);
    assert_eq!(loc1.to_string(), "src/main.cpp:10:5");
    assert_eq!(loc1.point.unwrap().line, 10);
    assert_eq!(loc1.point.unwrap().column, 5);

    let loc_file_only = SourceLocation::file_only("src/header.h");
    assert_eq!(loc_file_only.to_string(), "src/header.h");
    assert!(loc_file_only.point.is_none());

    let loc2 = SourceLocation::new("src/main.cpp", 10, 20);
    let range = SourceRange::spanned(loc1.clone(), loc2);
    assert_eq!(range.to_string(), "src/main.cpp:10:5-10:20");

    let single_range = SourceRange::single(loc1);
    assert_eq!(single_range.to_string(), "src/main.cpp:10:5");
}

#[test]
fn test_symbol_id_and_confidence_invariants() {
    let sym_id1 = SymbolId::clang_usr(Language::Cpp, "c:@F@foo#");
    let sym_id2 = SymbolId::clang_usr(Language::Cpp, "c:@F@foo#");
    let sym_id3 = SymbolId::clang_usr(Language::Cpp, "c:@F@bar#");

    // SymbolId 동등성 검증
    assert_eq!(sym_id1, sym_id2);
    assert_ne!(sym_id1, sym_id3);

    // Confidence는 오직 Confirmed, Possible, Unresolved만 존재 (FR-076: No PROBABLE)
    let conf1 = Confidence::Confirmed;
    let conf2 = Confidence::Possible;
    let conf3 = Confidence::Unresolved;
    assert_eq!(conf1.to_string(), "CONFIRMED");
    assert_eq!(conf2.to_string(), "POSSIBLE");
    assert_eq!(conf3.to_string(), "UNRESOLVED");
}

#[test]
fn test_symbol_query_parsing() {
    let q1 = SymbolQuery::parse("my_func");
    assert_eq!(q1.terminal_name, "my_func");
    assert_eq!(q1.qualifier_hint, None);

    let q2 = SymbolQuery::parse("ns::MyClass::method");
    assert_eq!(q2.terminal_name, "method");
    assert_eq!(q2.qualifier_hint, Some("ns::MyClass::".to_string()));
}

#[test]
fn test_call_kind_display() {
    assert_eq!(CallKind::Direct.to_string(), "direct");
    assert_eq!(CallKind::Virtual.to_string(), "virtual");
    assert_eq!(CallKind::FunctionPointer.to_string(), "function_pointer");
    assert_eq!(CallKind::Template.to_string(), "template");
    assert_eq!(CallKind::MacroExpanded.to_string(), "macro_expanded");
    assert_eq!(CallKind::Foreign.to_string(), "foreign");
    assert_eq!(CallKind::Unresolved.to_string(), "unresolved");
}
