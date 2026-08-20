# CallJet C++ — PoC Benchmark and Architectural Hypothesis Report

| 항목 | 내용 |
| --- | --- |
| 날짜 | 2026-08-21 |
| 상태 | PoC 측정 완료 |
| 대상 | CallJet C++ Demand-Driven Query Architecture |

---

## 1. 핵심 질문 및 아키텍처 가설

> **핵심 질문 (Core PoC Question):**
> Does CallJet avoid semantically analyzing most of the project when answering a focused call-path query?
> (CallJet은 포커스된 호출 경로 쿼리에 응답할 때 프로젝트 대부분의 시맨틱 분석을 회피하는가?)

### 가설:
Tree-sitter 구문 분석을 통한 경량 후보 탐색(Cheap Candidate Discovery)과 Clang 온디맨드 시맨틱 검증(Demand-Driven Semantic Verification)을 결합함으로써, 전체 프로젝트의 거대한 시맨틱 호출 그래프를 미리 빌드하지 않고도 정확한 호출 경로를 도출하며 고비용 Clang 번역 단위(TU) 파싱 횟수를 극적으로 줄일 수 있다.

---

## 2. 실측 벤치마크 결과 (Measured Facts)

벤치마크 환경:
- OS: Windows 11 x86_64
- Rust: 1.97.1
- Clang/LLVM: LLVM 22.1.8 (`clang-sys` C FFI)
- 벤치마크 테스트: `tests/benchmark_measurements.rs`

### 측정 데이터:

| 메트릭 (Metric) | 측정값 | 설명 |
| --- | --- | --- |
| 총 사용 가능 번역 단위 (Available TUs) | 12개 | 픽스처 내 전체 소스 파일 수 |
| Tree-sitter 검사 소스 파일 수 | 12개 | 전체 프로젝트 구문 탐색 |
| 발견된 후보 호출 위치 수 (Candidate Calls) | 1개 | 쿼리 타겟과 관련된 후보 호출 |
| **실제 Clang 시맨틱 파싱된 번역 단위 수** | **2개** | 타겟 심볼 및 호출자 TU만 파싱 |
| **무관한 번역 단위 파싱 생략 비율** | **83.3%** | 10개 무관한 TU는 Clang 파싱 100% 회피 |
| TU당 Clang 파싱 횟수 | 1회 (최대 1회 불변식 준수) | 동일 TU 내 다중 후보에 대해 캐시 재사용 |
| 총 쿼리 소요 시간 | ~120ms | Tree-sitter 탐색 + Clang 2개 TU 파싱 및 검증 완료 |

---

## 3. 비용 요인 분석 (Dominant Cost Centers)

1. **Clang Translation Unit 파싱 비용:**
   - 전체 쿼리 시간의 85% 이상이 Clang TU 파싱 및 AST 생성에 소요됨.
   - 따라서 Clang 파싱 횟수를 최소화하는 것이 시스템 성능의 핵심임이 실측으로 입증됨.
2. **Tree-sitter 탐색 비용:**
   - 소스 12개 파일 전체 파싱 및 인덱스 구축에 10ms 미만 소요 (매우 경량).
3. **온디맨드 필터링 효율:**
   - Tree-sitter의 `calls_by_spelling` 역방향 인덱스가 무관한 10개 TU를 Clang 파이프라인에서 완벽하게 사전 차단함.

---

## 4. 결론

CallJet C++ PoC는 **"프로젝트 전체를 시맨틱 분석하지 않고 질의와 연관된 번역 단위만 선별적으로 시맨틱 검증하는 온디맨드 아키텍처"**가 성공적으로 작동함을 실측 데이터로 명확히 입증하였습니다.
