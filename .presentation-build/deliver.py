from pathlib import Path
import json, zipfile, re, hashlib
from lxml import etree as E
from reportlab.pdfgen import canvas
from reportlab.lib.colors import HexColor
from reportlab.lib.utils import simpleSplit
from pypdf import PdfReader

root=Path(__file__).parent.parent
build=root/'.presentation-build'; out=root/'output'/'final'
deck=out/'Privantrix_Avanta_APSJRC.pptx'
audit=json.loads((build/'animation-audit.json').read_text())
ns={'p':'http://schemas.openxmlformats.org/presentationml/2006/main','a':'http://schemas.openxmlformats.org/drawingml/2006/main'}
with zipfile.ZipFile(deck) as z:
    slidefiles=[n for n in z.namelist() if re.fullmatch(r'ppt/slides/slide\d+.xml',n)]
    assert len(slidefiles)==12
    note_count=0
    for i in range(1,13):
        r=E.fromstring(z.read(f'ppt/notesSlides/notesSlide{i}.xml'))
        text=' '.join(r.itertext())
        assert all(v in text for v in ['OPENING','CORE EXPLANATION','TRANSITION'])
        note_count+=1
        r=E.fromstring(z.read(f'ppt/slides/slide{i}.xml'))
        assert r.find('p:timing',ns) is not None
        assert r.find('p:transition/p:fade',ns) is not None
        assert not r.findall('.//p:pic',ns)

pdf=canvas.Canvas(str(out/'Privantrix_Avanta_APSJRC.pdf'),pagesize=(960,540))
pdf.setTitle('Privantrix / Avanta - Continuous Software Assurance')
pdf.setAuthor('Privantrix / APS JRC')
for i in range(1,13):
    pdf.drawImage(str(build/'final-render-v2'/f'slide-{i}.png'),0,0,width=960,height=540)
    pdf.bookmarkPage(f'slide{i}')
    pdf.addOutlineEntry(f'{i:02d} - '+['Cover Slide','Problem & Opportunity','Existing Solutions & Gap Analysis','Our Solution','Technology & Innovation','Prototype / MVP / Demonstration','Target Users & Use Cases','Impact & Outcomes','Business Model & Implementation','Feasibility & Scalability','Competitive Advantage','Team & Vision'][i-1],f'slide{i}',0)
    if i==2:pdf.linkURL('https://survey.stackoverflow.co/2025/ai',(42,35,918,54),relative=0)
    pdf.showPage()
pdf.save()
assert len(PdfReader(out/'Privantrix_Avanta_APSJRC.pdf').pages)==12

qa=canvas.Canvas(str(out/'Privantrix_Avanta_APSJRC_Audit.pdf'),pagesize=(595.28,841.89))
qa.setTitle('Avanta - Visual and animation audit')
def heading(t,y):
    qa.setFillColor(HexColor('#142D46'));qa.setFont('Helvetica-Bold',20);qa.drawString(42,y,t)
def text(t,y,size=10.5,width=510):
    qa.setFillColor(HexColor('#25374A'));qa.setFont('Helvetica',size)
    for line in simpleSplit(t,'Helvetica',size,width):qa.drawString(42,y,line);y-=15
    return y-8
heading('Privantrix / Avanta',796)
y=text('Visual and animation audit | 5 September 2026',771,11)
y=text('Delivery: 12-slide editable PowerPoint and a 12-page static PDF. The original file in Downloads was not modified.',y)
heading('Content and visual review',y-8);y-=38
for para in [
    'All 12 slides were rendered and visually inspected. The final revision corrected the workflow label, priority-matrix spacing and comparison-table headings. The other ten slides are pixel-identical to their reviewed renders.',
    'Slide count, required section order, 16:9 dimensions, font consistency, package relationships and native tables passed automated checks. All slide content consists of editable text, shapes and native table rows. No slide screenshots are embedded in the PowerPoint.',
    'Every slide includes an opening line, core explanation, transition and presenter cues in speaker notes. Project maturity follows the supplied brief: R&D and experimentation now; integrated MVP next; pilot and enterprise deployment in the future.',
    'Slide 9 contains the commercial hypothesis, value equation, priority matrix and implementation path. Slide 10 pairs each risk with its proposed mitigation. Slide 11 labels Avanta capability ratings as design intent.',
    'The PDF preserves final visual states as high-resolution rendered pages. It is a static viewing copy; animations and editable objects remain in the PowerPoint.',
]:y=text(para,y)
heading('Limits of this audit',y-8);y-=38
for para in [
    'Native PowerPoint playback was not available. Animation timing and targets were checked in the file structure, not by running Slide Show in Microsoft PowerPoint. This audit does not establish playback compatibility on a particular competition computer.',
    'Motion uses the brief\'s Fade / Wipe fallback, not cross-slide Morph. Shapes remain editable. Presenter-controlled speaking beats reveal all content without exits, loops, sound, scripts or internet dependencies.',
    'Competition compliance was checked against the 12-section format in the supplied brief. No separate official rulebook was supplied. Compact matrices use approximately 12-point type, so their readability should be checked on the actual projector.',
]:y=text(para,y)
qa.showPage()
heading('Animation order',796)
y=text('46 speaking beats in total, plus normal slide advances. Supporting objects share a beat. Native fades last 0.30 seconds and wipes 0.35 seconds. Table rows stagger by 0.16 seconds.',765)
beats=[
 'Trust statement; team.',
 'Change flow; survey evidence; affected teams and opportunity.',
 'Tool functions; contextual gaps; common trust decision.',
 'Observe; understand; sandbox; verify; approve; learn and memory return.',
 'Source; context; Rudra; proposed action; verification; evidence and human gate.',
 'Concept and architecture; experimentation; future milestones; validation flow.',
 'Team size; qualifying conditions; primary users; workflows.',
 'Efficiency; verification; context and audit; baseline / pilot / target.',
 'Commercial model; value equation; priority matrix; implementation path.',
 'Build now; R&D required; scale later; paired risk and mitigation rows.',
 'Comparison rows; proposed differentiation; defensibility and final statement.',
 'Team; student-led vision; final statement and invitation.'
]
for row,description in zip(audit,beats):y=text(f"{row['slide']:02d}  ({row['clicks']} clicks)  {description}",y,10)
heading('Evidence and claim boundaries',y-5);y-=35
for para in [
    'Survey figures: Stack Overflow Developer Survey 2025, AI section. The three percentages describe different questions and must not be added. https://survey.stackoverflow.co/2025/ai',
    'Representative tool capabilities: official GitHub Copilot, Code Scanning and Actions documentation, plus OpenTelemetry documentation. Full source URLs appear in slide 3 and 11 speaker notes. Competitive LOW / MEDIUM / HIGH assessments are qualitative positioning judgments, not vendor-certified scores or benchmark results.',
    'No customers, revenue, pilots, accuracy figures, patents or completed production MVP are claimed. The value equation is a future measurement framework. Passing specified checks cannot prove the absence of every security flaw.',
]:y=text(para,y,9.5)
qa.save()
assert len(PdfReader(out/'Privantrix_Avanta_APSJRC_Audit.pdf').pages)==2
print(json.dumps({'slides':12,'notes':note_count,'speaking_beats':sum(x['clicks'] for x in audit),'animated_objects':sum(x['animated_objects'] for x in audit),'files':[p.name for p in out.iterdir()],'sha256':hashlib.sha256(deck.read_bytes()).hexdigest()}))
