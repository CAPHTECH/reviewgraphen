# ReviewGraphen Glossary

> Status: Draft v0.1  
> Rule: 用語は日常語の近似ではなく、ReviewGraphen内の操作的意味で使う

## A

### Accepted fact

入力sourceまたは明示的なauthorityによって、対象snapshot/context内で利用可能と認められた事実。正しさや安全性全体を意味しない。AI inferenceはconfidenceだけでaccepted factにならない。

### Abstention

reviewerがobligationへ結論を出さない構造化結果。`insufficient_context`、`unsupported_property`、`unresolved_symbol`、`policy_blocked`等の理由を持つ。失敗を`issue_absent`へ変換しないための第一級object。

### ArtifactSpace

レビュー対象世界を表すHigherGraphen Space。Code Review profileではProgramSpaceと呼ぶ。

## C

### Claim disposition

ReviewClaimの認識上の扱い。`proposed / supported / refuted / accepted / rejected / superseded`。obligation lifecycleやverification outcomeとは別軸。

### CompletionCandidate

不足している可能性があるtest、guard、API、invariant、evidence等の提案。受理されるまでaccepted structureではない。

### Context

語彙、契約、規則、証拠の有効性が局所的に成立する領域。例: UI event、payment、persistence、authentication。

### Context Cover

対象を複数のContextで被覆する構造。すべての重要対象とoverlapがcoverされているかを確認する。

### Coverage

定義済みuniverseに対して、どの段階まで処理されたかを測る値。単一percentageではなく、extraction、generation、visited、completed、evidence-supported、verified、fresh-verified等を分ける。

## D

### Decision

claim、finding、exception、plan等へauthorityが下す明示判断。accept/reject/exceptionとscope、理由、expiryを持つ。

### Deterministic fact

固定入力とtool versionに対して同じ結果を返す抽出・計算上の事実。parser output、hash、rule match等。deterministicであることは完全・正確であることと同義ではない。

## E

### Envelope

`ReviewContextEnvelope`の略。reviewerへ渡すbounded projection。

### Evidence

claimをsupports、refutes、qualifies、reproduces、contradicts、supersedesする観測物。source location、test result、trace、static result、human decision等。

### Evidence-backed coverage

required evidenceがbindingされたobligationのcoverage。evidenceの質やverification passを自動的には意味しない。

### EvidenceSpace

Evidenceとそのbinding、validity、provenanceを保持するSpace。

### Extraction completeness

ProgramSpace抽出が対象surfaceをどこまで扱えたか。file parse率、call resolution、data-flow capability、excluded region等。Review coverageとは別。

## F

### Finding

actionableなReviewClaimを、人間・CI向けに投影したobject。claim、evidence、verification、decisionへ追跡可能であることを要求する。未検証candidateは明示する。

### Fresh

current snapshot、profile、rule、policy、evidence、verifierとの依存関係が失効していない状態。

### Frontier

まだ処理可能で未完了のReviewObligation集合。schedulerが外部状態として管理する。

## G

### Gate

policyに基づき`pass / blocked / incomplete`を返す判断surface。findingの有無だけでなく、critical unknown、coverage、staleness、obstructionを考慮する。

### Gluing

Contextごとのlocal Sectionがoverlap上で両立し、global section/claimへ接続可能かを確認する操作。

### Gluing obstruction

local conclusions、assumptions、contracts、evidence environmentsがoverlapで両立せず、大域判断を構成できない理由。

### Graph-Driven Review

Program graphを単なるretrieval indexではなく、obligation生成、計画、context投影、coverage、evidence、stalenessを制御する外部状態として使う方法論。

## I

### Information loss

Projectionがsource structureから省略、統合、曖昧化したもの。kind、source count、reason、recoverability等を宣言する。

### Invariant

特定scope、context、changeを通じて保たれるべき性質。例: unauthenticated userからpersonal dataへのforbidden pathがない、同一orderへのchargeがat most once。

### Issue absent

指定propertyとEnvelope/evidence boundの範囲で問題を発見しなかったという限定claim。安全性全体の証明ではない。

## L

### Lifecycle

ReviewObligationの工程状態。`generated / planned / in_progress / completed / stale / superseded / cancelled`。

### Local claim

特定Context/Sectionに限定されたReviewClaim。global claimへ昇格するにはcoverとgluing条件を満たす必要がある。

## M

### Morphism

一つの構造から別の構造へのmapping。ReviewGraphenではsnapshot change、legacy migration、projection等を表し、preservation、loss、distortionを記録する。

## O

### Obligation Universe

固定snapshot、profile、rule set、extractor capability、policyに対して生成されたReviewObligationの分母。coverageは必ずこのdescriptorを参照する。

### Obstruction

確認、verification、gluing、sign-off、migration等が安全に成立しない構造化理由。artifact defectそのものとは限らない。

### Overlap

二つ以上のContextが共有する対象、契約、assumption、evidence scope。gluing時にrestrictionを比較する領域。

## P

### Path obligation

複数relationを通る実行、data、error、authorization、side-effect等のpathをtargetとするReviewObligation。

### Profile

対象domainのvocabulary、required extractor capabilities、rule packs、evidence policy、projection、gate policyをversionedに束ねた定義。

### ProgramSpace

Code Review profileにおけるArtifactSpace。files、symbols、relations、paths、tests、configs、invariants等をaccepted/inferred境界付きで表す。

### Projection

Higher structureから特定audience/purpose向けviewを生成するmorphism。source traceとinformation lossを持つ。

### Property

obligationが確認する性質のversioned identifier。自然言語promptだけにしない。例: `payment.idempotency`、`error.propagation`。

## R

### Relation obligation

call、read/write、ownership、serialization、test mapping等のrelation自体をtargetとするReviewObligation。

### ReviewClaim

reviewerがobligationに対して生成する候補判断。polarity、scope、source grounding、limitationsを持つ。LLM出力は原則proposed claim。

### ReviewContextEnvelope

一つまたは強く結合した少数obligationを判断するために、ProgramSpace/EvidenceSpaceから構成したbounded projection。

### ReviewDecision

claim/finding/exception等に対する明示的authority判断。confidenceでは代替しない。

### ReviewExecution

reviewer、Envelope、configuration、tool use、raw output、claim/abstentionを結ぶ実行記録。

### Review Graph

ReviewSpaceを中心に、obligations、plans、executions、claims、evidence bindings、decisions、coverage、obstructions、stalenessを関係づけた成果物。

### ReviewObligation

target、property、context requirement、evidence requirement、risk、applicability、version、provenanceを持つ確認責務。ReviewGraphenの中心object。

### ReviewPlan

obligation依存、risk、budget、available reviewer/verifierを考慮した実行候補。planのacceptanceとexecutionを分ける。

### ReviewProfile

Profileと同義。review固有のversioned interpretation package。

### ReviewSpace

review工程の認識状態を保持するSpace。

### Reviewer

obligationを処理してclaim/abstentionを返すactor。LLM、static analyzer、formal tool、human等。reviewerの種類はevidenceの強さと同義ではない。

### Risk-weighted coverage

obligationごとのrisk weightを考慮したcoverage。raw coverageを置き換えず併記する。risk scoreはseverityの確定値ではない。

## S

### Section

Context内で成立するlocal assignment。contract、assumption、claim、evidence environment等を表す。

### Semantic key

snapshotをまたぐ対応づけに使う、IDとは別の意味的識別子。rename/move、split/mergeへ利用するが、対応不能ならstaleにする。

### Source grounding

claimが具体的なsource cells、relations、paths、locationsへ結びついていること。groundingはclaimの正しさを単独で保証しない。

### Stale

参照source、dependency、context、evidence、rule、policy等が変わり、current判断へ再利用できない状態。

### Subgraph obligation

feature、bounded context、pipeline等の複数cell/relationからなる構造をtargetとするReviewObligation。

## U

### Unknown

能力不足、情報不足、mapping不能等により判定できない領域。zero findingやpassへ変換しない。

### Unreviewed

明示review decisionがまだない状態。AI-generated candidateのdefault。

### Unverifiable

現在利用可能なverifier/evidence policyではclaimを検証できない状態。`issue_absent`や`passed`とは異なる。

## V

### Verification

claimを指定verifierとevidence boundで確認した結果。`passed / failed / inconclusive / unsupported / expired`等。claim acceptanceとは別。

### Verified coverage

verification policyを満たしたobligationのcoverage。current gateにはfresh verified coverageを使う。

### Visited coverage

reviewer executionが開始され、対象が実際に処理されたobligationのcoverage。完了、evidence、verificationを意味しない。

## W

### Witness

claim、violation、preservation、gluing等を支持または反証する具体的観測。counterexample pathやfailing testを含む。
