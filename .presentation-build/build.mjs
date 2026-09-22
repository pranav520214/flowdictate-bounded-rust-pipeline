import fs from 'node:fs/promises';
import path from 'node:path';
import { Presentation, PresentationFile } from '@oai/artifact-tool';

const ROOT='C:/Users/RYZEN/Desktop/automation for';
const OUT=path.join(ROOT,'.presentation-build');
const p=Presentation.create({slideSize:{width:1280,height:720}});
const C={bg:'#0B111A',panel:'#111E2C',ink:'#F5F8FC',muted:'#BDCADA',cyan:'#52D5F2',line:'#344A60',red:'#E18D91',green:'#8ED7BC'};
const F='Arial';
const master=p.masters.add('Privantrix Dark');
const layout=p.layouts.add('Avanta 16:9');layout.setParentLayoutId(master.id);
let s,beat=0,serial=0;
const titles=['Cover Slide','Problem & Opportunity','Existing Solutions & Gap Analysis','Our Solution','Technology & Innovation','Prototype / MVP / Demonstration','Target Users & Use Cases','Impact & Outcomes','Business Model & Implementation','Feasibility & Scalability','Competitive Advantage','Team & Vision'];
const sourceSO='https://survey.stackoverflow.co/2025/ai';
const sourceTools=['https://docs.github.com/en/copilot/concepts/agents/cloud-agent/about-cloud-agent','https://docs.github.com/en/code-security/concepts/code-scanning/code-scanning','https://docs.github.com/en/actions/get-started/understand-github-actions','https://opentelemetry.io/docs/what-is-opentelemetry/'];
function box(x,y,w,h,fill=C.panel,line='none',name='box',geom='rect'){
 return s.shapes.add({name:`b${beat}_${name}_${++serial}`,geometry:geom,position:{left:x,top:y,width:w,height:h},fill,line:{fill:line,width:line==='none'?0:1.5}});
}
function txt(t,x,y,w,h,size=24,color=C.ink,bold=false,name='text',align='left'){
 const sh=box(x,y,w,h,'none','none',name,'textbox');sh.text=t;
 sh.text.style={fontSize:size,typeface:F,color,bold,alignment:align,verticalAlignment:'middle',autoFit:'none',wrap:true};return sh;
}
function line(x,y,w,color=C.line,h=2){return box(x,y,w,h,color,'none','flow');}
function tag(t,x,y,w=600,color=C.cyan){return txt(t,x,y,w,26,16,color,true);}
function para(t,x,y,w,h=66,size=23){return txt(t,x,y,w,h,size,C.muted);}
function stage(n){beat=n;}
function start(n,headline){s=p.slides.add();s.setLayout(layout);s.background.fill=C.bg;beat=0;
 tag(`${String(n).padStart(2,'0')}  /  ${titles[n-1].toUpperCase()}`,56,30,1168);
 if(headline)txt(headline,56,78,1168,108,42,C.ink,true,'headline');
 line(56,670,1168);txt('PRIVANTRIX / AVANTA',56,681,700,20,13,C.muted);txt(String(n).padStart(2,'0'),1180,679,44,23,14,C.cyan,true,'page','right');return s;
}
function notes(opening,explanation,transition,extra='',sources=[]){s.speakerNotes.textFrame.setText(`OPENING\n${opening}\n\nCORE EXPLANATION (about 25 seconds)\n${explanation}\n\nTRANSITION\n${transition}\n\nPRESENTER CUES\n${extra}\n\nSTATUS AND SOURCES\nProject facts and proposed plans: user-supplied redesign brief and original Privantrix_Avanta_APSJRC.pptx. Project status is team-reported, not independently validated.\n${sources.join('\n')}\nSources checked 5 September 2026. Native entrance builds preserve every item in the final slide state.`);}
function node(title,desc,x,y,w,number,color=C.cyan){line(x,y,w,color,3);tag(number,x,y+15,w,color);txt(title,x,y+52,w,36,w<190?20:23,C.ink,true);para(desc,x,y+97,w,95,w<190?19:20);}
function table(values,x,y,width,height,cols,fontsize=18){const t=s.tables.add({rows:values.length,columns:values[0].length,left:x,top:y,width,height,columnWidths:cols,values});
 t.borders.assign({fill:C.line,width:0.65,style:'solid'});
 for(let r=0;r<values.length;r++){t.rows[r].height=height/values.length;for(let c=0;c<values[0].length;c++){const ce=t.getCell(r,c);ce.fill=r===0?C.panel:C.bg;ce.text.style={typeface:F,fontSize:fontsize,color:c===values[0].length-1?C.cyan:C.ink,bold:r===0,alignment:c===0?'left':'center',autoFit:'none'};}}
 return t;
}

// 01: spare typographic cover with an editable trust-gate motif.
start(1,'');tag('PRIVANTRIX',56,118,610);txt('AVANTA',52,163,710,125,108,C.ink,true);
txt('Continuous Software Assurance',56,306,710,48,33,C.cyan);
stage(1);line(809,160,3,C.cyan,370);txt('TRUST',855,170,370,86,68,C.ink,true);txt('EVERY',855,260,370,86,68,C.ink,true);txt('CHANGE.',855,350,370,86,68,C.cyan,true);
para('A private trust runtime concept for\nevidence-driven software change.',56,386,705,92,27);
tag('CURRENTLY IN RESEARCH & DEVELOPMENT',56,508,730);
stage(2);txt('Pranav Kumar Mishra',56,578,385,29,21,C.ink,true);para('Project Lead',56,610,385,28,17);
txt('Aryan Kashyap',458,578,300,29,21,C.ink,true);para('Co-Lead',458,610,300,28,17);
txt('Gagan Chaddah',823,578,320,29,21,C.ink,true);para('Mentor  /  APS JRC',823,610,340,28,17);
notes('We are building Avanta, a concept for continuous software assurance.','When a software change reaches review, teams need more than a suggested fix. Our proposed system gathers the relevant evidence and makes the human decision easier to inspect. We are a student-led team at APS JRC. Today we are in research, architecture design and model/workflow experimentation, with an integrated MVP still ahead.','The starting point is a gap between AI adoption and trust.','2 clicks: trust statement, then team. Allow about 25 seconds.');

// 02: evidence + a visible review bottleneck.
start(2,'AI adoption is high. Trust still needs evidence.');
stage(1);tag('SOFTWARE CHANGE',56,205,400);line(56,255,625,C.cyan,3);
for(let i=0;i<6;i++)box(82+i*81,239,48,32,C.panel,C.cyan,'commit');
box(711,215,3,94,C.red);txt('REVIEW / TRUST?',745,226,470,58,36,C.ink,true);
stage(2);const stats=[['84%','use or plan to use AI\nin development'],['46%','distrust the accuracy\nof AI tool output'],['66%','cite almost-right\nAI answers as a frustration']];
stats.forEach((a,i)=>{const x=56+i*397;txt(a[0],x,329,365,87,70,C.cyan,true);para(a[1],x,424,355,70,23);});
stage(3);tag('WHO FEELS THE PRESSURE',56,521,450);para('Developers reviewing AI-generated PRs, startup CTOs and engineering leads with limited AppSec capacity.',56,552,1168,60,24);
txt('Opportunity: one inspectable trust decision for each change',56,616,1168,32,26,C.ink,true);
stage(0);txt('Source: Stack Overflow Developer Survey 2025, AI section. Self-reported responses to different questions.',56,650,1168,18,13,C.muted);
notes('AI adoption has grown faster than confidence in its output.','Stack Overflow’s 2025 survey reports that 84% use or plan to use AI tools, while 46% distrust their accuracy. Another question found that 66% were frustrated by almost-right answers. These are self-reported survey figures, not a benchmark of security failures. Our opportunity is to bring evidence together at the review decision, especially for teams with limited security capacity.','Several tools already help, but their signals still need to meet.','3 clicks: change flow, survey evidence, affected teams and opportunity. Percentages have different question populations and should not be added.',[sourceSO]);

// 03: four tools converge on a shared decision.
start(3,'Useful tools. A fragmented trust decision.');
const toolsData=[['Coding agents','Generate, explain and\nsuggest changes','Output still needs checks\nagainst project intent'],['Security scanners','Find risky patterns\nand dependencies','Findings may need\ncontext and human triage'],['CI/CD testing','Build, test and\ncontrol release','Coverage depends on\nthe checks teams define'],['Observability','Collect runtime\ntelemetry','Runtime signals may lack\nchange-level context']];
stage(1);toolsData.forEach((a,i)=>{const x=56+i*298;line(x,213,274,C.cyan,3);txt(a[0],x,239,274,35,24,C.ink,true);para(a[1],x,291,274,74,23);});
stage(2);toolsData.forEach((a,i)=>{const x=56+i*298;para(a[2],x,382,274,69,20);line(x+137,464,2,C.line,48);});
stage(3);line(193,511,894,C.line,2);line(638,512,2,C.cyan,31);txt('TRUST DECISION',365,546,550,39,28,C.cyan,true,'decision','center');
txt('Proposed unmet need: a private assurance layer before release',56,609,1168,38,29,C.ink,true);
stage(0);txt('Sources: GitHub Copilot, Code Scanning and Actions documentation; OpenTelemetry overview. Links in notes.',56,650,1168,18,13,C.muted);
notes('Each category solves a useful part of the problem.','Coding agents can generate changes and run tests. Scanners find vulnerable patterns, CI runs defined checks, and observability provides runtime signals. Their capabilities overlap, so our claim is a workflow gap rather than an absence of features. Avanta would bring these signals into one private evidence packet, tied to a specific software change and a human decision.','That gives us the six-stage loop we want to build.','3 clicks: tool functions, contextual gaps, shared decision. Gaps are team interpretations of workflow needs, not universal product limitations.',sourceTools);

// 04: hero six-step architecture.
start(4,'Evidence before trust');tag('PROPOSED AVANTA TRUST LOOP',56,190,1168);
const loop=[['OBSERVE','Code, dependencies\nand change signals'],['UNDERSTAND','Repository and\nproject context'],['SANDBOX','Isolated execution\nwhere required'],['VERIFY','Compiler, tests\nand scanners'],['APPROVE','Evidence packet\nand human gate'],['LEARN','Validated evidence\ninto private memory']];
loop.forEach((a,i)=>{stage(i+1);node(a[0],a[1],56+i*197,281,177,String(i+1).padStart(2,'0'));if(i<5)line(233+i*197,330,20,C.cyan,2);});
stage(6);line(143,509,984,C.line,2);line(143,485,2,C.line,24);line(1127,485,2,C.line,24);tag('VERIFIED-WRITE MEMORY FEEDS FUTURE CONTEXT',230,526,850);
txt('Pull request → context → evaluation → proposed action → verification → evidence → human decision',56,589,1168,55,23,C.ink);
notes('Avanta would turn a pull request into an evidence-backed decision.','We observe the change, assemble repository context, and use isolated execution where the risk requires it. Verification then runs the defined compiler, test and security checks. The evidence packet reaches a human for approval or rejection. Only validated evidence can update private project memory. The learning step must preserve provenance rather than accepting unverified model output.','The architecture separates what the model proposes from what the gates can verify.','6 clicks, one per stage. The final beat adds the memory return line and user journey. This is a proposed workflow, not an operating product.');

// 05: two domains separated by a verification boundary.
start(5,'Reason probabilistically. Verify deterministically.');tag('PROPOSED AVANTA ARCHITECTURE / CURRENTLY UNDER DEVELOPMENT',56,187,1168);
stage(1);tag('SOURCE',56,244,165);txt('Code change\nPull request',56,289,172,75,26,C.ink,true);line(216,328,29,C.cyan);
stage(2);box(255,240,390,226,C.panel);tag('CONTEXT LAYER',277,256,345);para('Repository structure / dependencies\nProgram graph / security findings\nHistorical project memory',277,303,345,101,20);txt('Structured context packet',277,422,345,31,22,C.cyan,true);
stage(3);line(450,466,2,C.cyan,16);box(255,482,390,90,C.panel,C.line);tag('RUDRA REASONING',277,493,345);txt('Interpret context. Propose a patch.',277,529,345,32,21,C.ink);
stage(4);line(656,240,3,C.line,332);line(645,527,30,C.cyan);line(674,315,2,C.cyan,214);line(674,315,65,C.cyan);tag('PROPOSED CHANGE',693,239,510);para('A candidate action crosses the gate boundary',693,275,510,35,21);
stage(5);box(693,331,531,133,C.panel,C.cyan);tag('DETERMINISTIC VERIFICATION',712,345,494);para('Compile / unit + integration tests / SAST\nDependency checks / security behaviour\nSandbox execution where required',712,381,494,78,20);
stage(6);txt('EVIDENCE PACKET',693,501,250,35,24,C.cyan,true);line(944,521,27,C.cyan);txt('HUMAN GATE',985,501,239,35,24,C.ink,true);para('Approve or reject against defined policy',693,549,531,35,23);
txt('Models propose. Evidence controls the trust decision.',56,613,1168,38,30,C.ink,true);
notes('Avanta separates reasoning from trust.','The context layer would collect repository structure, dependencies, findings and project history into a structured packet. Rudra would interpret that packet and propose a minimal action. The proposal must then pass defined verification gates before a human evaluates the evidence. Passing those checks only establishes the properties tested. It cannot prove that every possible security issue is absent.','This is the design we are preparing. Here is where development stands.','6 clicks: source, context, Rudra, candidate action, verification, evidence and human gate. Gate labels are architectural symbols, not measured test successes.');

// 06: maturity and next controlled experiment.
start(6,'Architecture first. Integrated MVP next.');tag('CURRENT STATUS: RESEARCH & DEVELOPMENT',56,189,1168);
const maturity=[['Research','Concept defined','COMPLETED'],['Architecture','Context + gate design','IN PROGRESS'],['Fine-tuning','Model / workflow trials','IN PROGRESS'],['Integrated MVP','End-to-end loop','NEXT'],['Controlled pilot','External validation','FUTURE']];
maturity.forEach((a,i)=>{stage(i<2?1:i===2?2:3);node(a[0],a[1],56+i*237,243,214,a[2],i>2?C.muted:C.cyan);});
stage(4);tag('PLANNED MVP VALIDATION FLOW',56,468,1168);txt('Safe public / synthetic repo → controlled vulnerability → context → proposed change',56,514,1168,41,25,C.ink);txt('Verification gates → evidence report → human approve / reject',56,562,1168,41,25,C.cyan,true);
txt('Target: accept a change only after the required gates pass',56,619,1168,35,28,C.ink,true);
notes('We are currently in research and development.','We have defined the problem and the high-level assurance concept. Detailed architecture, context-packet design, model evaluation and fine-tuning experimentation are in progress. We are also planning the integrations. The next milestone is one end-to-end MVP on a safe public or synthetic repository with a controlled vulnerability. A pilot comes later, after that workflow produces evidence we can inspect.','The first users should have a clear need for that narrow workflow.','4 clicks: concept and architecture, experimentation, future milestones, planned validation. Research completed means initial problem and concept research, not that all research has ended. The MVP and validation outcome remain planned.');

// 07: proposed users and common assurance layer.
start(7,'Frequent releases. Limited AppSec capacity.');stage(1);tag('PROPOSED BEACHHEAD SEGMENT',56,201,630);txt('10–200',56,252,650,119,102,C.cyan,true);txt('engineers per team',56,385,630,44,34,C.ink,true);
stage(2);para('Frequent releases\nAI-assisted development\nLimited dedicated AppSec staff',56,459,570,123,28);
stage(3);tag('PRIMARY USERS',739,204,485);para('Startup CTO / engineering lead\nSenior developer / security-minded founder\nSmall AppSec team',739,245,485,102,22);
stage(4);tag('WORKFLOWS',739,384,485);para('AI-generated PR / dependency upgrade\nSecurity patch / release candidate\nOnboarding and engineering context',739,427,485,108,23);
line(693,205,2,C.line,377);txt('One assurance layer, with private context across reviews',56,612,1168,40,30,C.ink,true);
notes('We propose starting with software teams of roughly ten to two hundred engineers.','That is a target hypothesis, not a customer count. The qualifying signals are frequent releases, AI-assisted development and limited dedicated AppSec capacity. The daily user might be a senior developer or engineering lead, while a CTO could evaluate the purchase. We would begin with PR assurance and evidence aggregation, then test whether the same context helps upgrades and release reviews.','We would measure value before making claims about impact.','4 clicks: team size, qualifying conditions, users, workflows. Expected benefits include less tool switching, faster triage, fewer blind approvals and stronger auditability. No customer adoption is claimed.');

// 08: native measurement table with no fictional results.
start(8,'Success needs a baseline and an evidence trail');tag('PLANNED VALIDATION METRICS / NO RESULTS CLAIMED',56,188,1168);
stage(1);tag('TIME / EFFICIENCY',56,249,363);txt('Less review effort',56,291,363,42,29,C.ink,true);para('Triage minutes: finding to decision\nReview hours per repository\nTool switches and manual handoffs',56,353,363,130,22);
stage(2);tag('SECURITY / VERIFICATION',455,249,363);txt('More complete evidence',455,291,363,42,29,C.ink,true);para('Required gates satisfied (%)\nUnsafe dependency exposure time\nFailures, retries and false approvals',455,353,363,130,22);
stage(3);tag('CONTEXT / AUDIT',854,249,370);txt('Traceable decisions',854,291,370,42,29,C.ink,true);para('Evidence-packet completeness\nValidated context preserved\nDecision and memory provenance',854,353,370,130,22);
stage(4);line(56,517,1168,C.cyan,2);tag('BASELINE',56,542,260);tag('PILOT',456,542,260);tag('TARGET',854,542,300);para('Measure the current workflow',56,578,370,37,21);para('Repeat on comparable changes',456,578,370,37,21);para('Set after baseline collection',854,578,370,37,21);
txt('Desired outcome: faster decisions with stronger evidence',56,624,1168,31,27,C.ink,true);
notes('These are measurement categories, not results we have achieved.','For the planned MVP, we would record triage time, review effort and verification outcomes on controlled cases. We also need evidence quality, memory provenance and the cost of failures or retries. Later pilots should compare similar changes against a baseline, using the same definitions. A faster result only counts as progress if evidence quality and rejection of unsafe changes remain acceptable.','Those measurements can become a business-value framework.','4 clicks: efficiency, verification, context, baseline-to-pilot method. Gate satisfaction rate = proposals passing every required gate divided by all evaluated proposals. No target value has been invented.');

// 09: commercial hypothesis, value equation, matrix and staged implementation.
start(9,'A narrow workflow must earn its expansion');stage(1);tag('PROPOSED COMMERCIAL MODEL / REQUIRES CUSTOMER VALIDATION',56,184,1168);
[['STARTUP / PILOT','Per repository or app','Scanners + evidence + approval'],['TEAM','Per repo / workload / developer','CI + private memory + policy'],['ENTERPRISE / FUTURE','Private deployment + support','VPC / on-prem + audit controls']].forEach((a,i)=>{const x=56+i*397;tag(a[0],x,225,374);txt(a[1],x,261,374,34,22,C.ink,true);para(a[2],x,303,374,30,19);});
stage(2);line(56,351,1168);tag('PILOT VALUE FRAMEWORK',56,370,560);txt('Review hours saved × cost per hour\n+ avoided remediation / delay cost\n= estimated customer value',56,410,551,108,27,C.ink,true);
para('Measure compute cost per analysis, cost per verified change, retries and infrastructure cost.',56,539,549,59,19);
stage(3);tag('STRATEGIC PRIORITY MATRIX',677,368,547);box(709,416,515,177,C.panel);line(954,416,2,C.line,99);line(709,515,515,C.line);txt('LOWER READINESS',722,425,219,25,16,C.muted,true);txt('INITIAL PRIORITY',968,425,242,25,16,C.cyan,true);
txt('Autonomous verified repair\nMulti-repo memory\nPolicy intelligence',722,463,219,52,16,C.ink);
txt('AI PR assurance\nSecurity evidence aggregation\nCI / release verification',968,461,245,54,16,C.ink);
txt('Lower priority: general coding assistant,\nobservability replacement, project management',722,539,485,42,16,C.muted);
txt('Value: low → high ↑',649,596,244,23,15,C.muted);txt('Technical readiness: low → high',919,596,305,23,15,C.muted);
stage(4);txt('CURRENT  Research + architecture     MVP  Single repo     INTEGRATION  CI + sandbox',56,609,1168,23,18,C.cyan,true);
txt('TEAM RUNTIME  Private memory + policy     ENTERPRISE FUTURE  Deployment + audit governance',56,634,1168,23,17,C.cyan,true);
stage(0);txt('Measurement framework, not financial performance. Proposed sequence then adds team memory and enterprise governance.',56,652,1168,17,12.8,C.muted);
notes('Our commercial model is a hypothesis that needs customer validation.','The initial offering could charge per repository for evidence and approval workflows. Team and private enterprise deployments would come later. We would estimate customer value from review hours saved times engineering cost, plus attributable avoided remediation or delay cost. We also have to measure compute and verification costs. The priority matrix keeps the first product focused on PR assurance and evidence aggregation.','The same focus makes the technical build more manageable.','4 clicks: commercial model, value equation, priority matrix, implementation path. Treat avoided cost cautiously: estimate only with evidence and avoid double counting review time. Net customer benefit subtracts subscription and adoption costs. Contribution potential subtracts compute, infrastructure and support costs from revenue; none are measured yet. Roadmap: current research and architecture; single-repo MVP; CI, sandbox and verification integration; team runtime with private memory and policy; future enterprise deployment with audit governance. Matrix positions are team hypotheses, not validated market scores.');

// 10: build feasibility plus native risk/mitigation rows.
start(10,'The assurance loop comes before the platform');
stage(1);tag('BUILD NOW',56,195,358);para('Scanners / CI hooks / repository APIs\nContainers / compilers / tests\nStructured evidence reports',56,235,358,94,21);
stage(2);tag('R&D REQUIRED',458,195,358);para('Context quality / memory safety\nModel choice / fine-tuning / graphs\nSecure access / gate orchestration',458,235,358,94,21);
stage(3);tag('SCALE LATER',861,195,363);para('Single repository → team workspace\nMulti-repository memory\nPrivate enterprise runtime',861,235,363,94,21);
stage(4);tag('RISKS / PROPOSED MITIGATIONS',56,354,1168);
table([
 ['Risk','Potential impact','Proposed mitigation'],
 ['False positives','Wasted review time','Correlate signals + human triage'],
 ['Incorrect model patch','Unsafe or broken change','Compiler + tests + security gates'],
 ['Memory poisoning','Incorrect future context','Verified-write policy + provenance'],
 ['Sensitive code exposure','Loss of confidentiality','Private/local design + scoped access'],
 ['Over-automation','Unsafe autonomous decisions','Human approval for high-risk actions'],
 ['Compute cost','Poor unit economics','Selective models + bounded workflows']
],56,392,1168,221,[278,322,568],17);
txt('Automate only what can remain verifiable',56,628,1168,31,29,C.ink,true);
notes('The first assurance loop can use existing components, but integration is still research work.','Scanners, compilers, containers and CI hooks give us a starting point. The hard parts include context quality, safe private memory and verification orchestration. Each risk needs a control: human triage for noisy findings, defined checks for patches, provenance for memory, and bounded work for compute cost. Private deployment also needs secure access controls. These are proposed mitigations, not solved risks.','That leaves a specific differentiation hypothesis to validate.','4 clicks: build now, research, scale later, risk pairs. Risk and mitigation rows reveal together. Include test incompleteness as a limitation: passing tests does not establish universal security.');

// 11: cautious comparison with current feature overlap acknowledged.
start(11,'Proposed differentiation: private context + verification');tag('DIRECTIONAL POSITIONING / AVANTA COLUMN SHOWS DESIGN INTENT',56,187,1168);
stage(1);table([
 ['Capability','Coding agents','Security tools','CI/CD','Observability','AVANTA'],
 ['Generate / suggest fixes','HIGH','MEDIUM','LOW','LOW','MEDIUM'],
 ['Private repository context','HIGH','HIGH','HIGH','MEDIUM','HIGH'],
 ['Persistent engineering memory','MEDIUM','MEDIUM','LOW','MEDIUM','HIGH'],
 ['Verification workflow','HIGH','HIGH','HIGH','MEDIUM','HIGH'],
 ['Human approval','HIGH','HIGH','HIGH','MEDIUM','HIGH'],
 ['Cross-layer evidence','MEDIUM','MEDIUM','HIGH','HIGH','HIGH'],
 ['Pre-release assurance','HIGH','HIGH','HIGH','MEDIUM','HIGH'],
 ['Auditability','HIGH','HIGH','HIGH','HIGH','HIGH']
],56,229,1168,270,[348,160,166,121,187,186],16);
stage(2);txt('PRIVATE CONTEXT  +  EVIDENCE  +  VERIFICATION  +  HUMAN GATE',56,529,1168,34,25,C.cyan,true);
stage(3);tag('POTENTIAL FUTURE DEFENSIBILITY',56,575,470);para('Orchestration depth, verified memory rules and accumulated engineering context',533,576,691,28,17);
txt('Every AI-generated and human change must present evidence',56,611,1168,34,26,C.ink,true);
stage(0);txt('Team assessment of solution positioning. Capabilities vary by product. Deeper validation planned. Sources in notes.',56,650,1168,18,12.5,C.muted);
notes('Our differentiation is proposed, and there is substantial competitive overlap.','Coding agents already support repository context and testing, while CI and security tools provide strong verification and approval features. This directional matrix describes emphasis, not measured product performance. Avanta’s hypothesis is that private project context, cross-layer evidence and an explicit human gate can work well together in one assurance runtime. Any defensibility would have to emerge from that integration and validated memory practices.','Our team is building the foundations needed to test that hypothesis.','3 clicks: comparison, four-part proposed wedge, potential defensibility and closing claim. LOW/MEDIUM/HIGH are qualitative team judgments, not scores established by cited vendors. Avanta ratings are target design intent, not delivered capability. No patents or established IP claimed. Sources establish representative feature overlap, not numerical rankings.',sourceTools);

// 12: authentic team and static final statement.
start(12,'Safer software should emerge from every change');
stage(1);[['Pranav Kumar Mishra','Project Lead','Architecture / product vision\nResearch direction'],['Aryan Kashyap','Co-Lead','Engineering support / prototype\nTesting workflow'],['Gagan Chaddah','Mentor','Guidance / validation\nPresentation review']].forEach((a,i)=>{const x=56+i*397;line(x,218,374,C.cyan,3);txt(a[0],x,244,374,36,26,C.ink,true);tag(a[1].toUpperCase(),x,292,374);para(a[2],x,337,374,73,21);});
stage(2);tag('STUDENT-LED / RESEARCH-DRIVEN / APS JRC',56,446,1168);para('We are researching and building the foundations of software that stays secure, understandable, maintainable and verifiable as code, teams and threats evolve.',56,484,1168,68,25);
stage(3);txt('TRUST EVERY CHANGE.',56,566,1168,62,50,C.cyan,true);
txt('Seeking technical mentors, validation feedback and safe repositories. Design partners after MVP validation.',56,637,1168,28,18,C.muted);
notes('We are a student-led team at APS JRC.','Pranav leads architecture, product vision and research. Aryan supports engineering, prototype development and testing. Gagan Chaddah mentors the team through guidance and review. Our vision is engineering intelligence that helps software remain secure and understandable as it changes. We are seeking technical mentorship, validation feedback and safe public or synthetic repositories. Future design partners would come after MVP validation.','Trust every change.','3 clicks: team, student-led vision, final statement and invitation. Hold the final frame. Total core speaking time is approximately six minutes with normal pauses.');

await fs.mkdir(OUT,{recursive:true});
await (await PresentationFile.exportPptx(p)).save(path.join(OUT,'draft.pptx'));
await fs.writeFile(path.join(OUT,'deck.proto.json'),JSON.stringify(p.toProto()));
await fs.mkdir(path.join(OUT,'draft-render'),{recursive:true});
for(let i=0;i<p.slides.items.length;i++){const slide=p.slides.items[i];const blob=await p.export({slide,format:'png',scale:1});await fs.writeFile(path.join(OUT,'draft-render',`slide-${i+1}.png`),new Uint8Array(await blob.arrayBuffer()));}
console.log('Created editable 12-slide draft and previews.');
