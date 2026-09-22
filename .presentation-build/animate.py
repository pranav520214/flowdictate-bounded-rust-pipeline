from pathlib import Path
from copy import deepcopy
from collections import defaultdict
import re,json,zipfile
from lxml import etree as E

ROOT=Path(__file__).parent
NS={'p':'http://schemas.openxmlformats.org/presentationml/2006/main','a':'http://schemas.openxmlformats.org/drawingml/2006/main'}
P=NS['p']; A=NS['a']
def el(parent,name,**attrs): return E.SubElement(parent,'{%s}%s'%(P,name),{k:str(v) for k,v in attrs.items()})
def condition(parent,delay='0'):
    lst=el(parent,'stCondLst');el(lst,'cond',delay=delay)
def table_rows(root,beat):
    tree=root.find('p:cSld/p:spTree',NS)
    maxid=max(int(n.get('id')) for n in root.findall('.//p:cNvPr',NS))
    for gf in list(tree.findall('p:graphicFrame',NS)):
        tab=gf.find('.//a:tbl',NS)
        if tab is None: continue
        rows=tab.findall('a:tr',NS)
        if len(rows)<2: continue
        yy=int(gf.find('p:xfrm/a:off',NS).get('y')); insert=tree.index(gf)
        tree.remove(gf)
        for idx,row in enumerate(rows):
            f=deepcopy(gf); t=f.find('.//a:tbl',NS)
            for r in list(t.findall('a:tr',NS)):t.remove(r)
            t.append(deepcopy(row));h=int(row.get('h'))
            f.find('p:xfrm/a:off',NS).set('y',str(yy));f.find('p:xfrm/a:ext',NS).set('cy',str(h));yy+=h
            maxid+=1;n=f.find('p:nvGraphicFramePr/p:cNvPr',NS);n.set('id',str(maxid));n.set('name',f'b{beat}_table_row_{idx}')
            # Creation ids must remain unique after splitting a native table.
            for ext in list(n):n.remove(ext)
            tree.insert(insert+idx,f)

def animate(root,slide_number):
    groups=defaultdict(list)
    for n in root.findall('.//p:cNvPr',NS):
        m=re.match(r'b(\d+)_(.*)',n.get('name',''))
        if m and int(m[1])>0:groups[int(m[1])].append((n.get('id'),m[2]))
    # Normal PowerPoint Fade transition; object continuity plus native Wipe is the fallback motion system.
    tr=el(root,'transition',spd='fast',advClick='1');el(tr,'fade')
    timing=el(root,'timing');lst=el(timing,'tnLst');par=el(lst,'par')
    counter=1
    def tn(parent,**attrs):
        nonlocal counter
        r=el(parent,'cTn',id=counter,**attrs);counter+=1;return r
    top=tn(par,dur='indefinite',restart='never',nodeType='tmRoot');children=el(top,'childTnLst')
    seq=el(children,'seq',concurrent='1',nextAc='seek');main=tn(seq,dur='indefinite',nodeType='mainSeq');ml=el(main,'childTnLst')
    entries=[]
    for group,items in sorted(groups.items()):
        outer=tn(el(ml,'par'),fill='hold');condition(outer,'indefinite');ol=el(outer,'childTnLst')
        beatnode=tn(el(ol,'par'),fill='hold');condition(beatnode);bl=el(beatnode,'childTnLst')
        for idx,(sid,name) in enumerate(items):
            filt='wipe(right)' if name.startswith('flow') else 'fade'
            delay=0
            rm=re.search(r'table_row_(\d+)',name)
            if rm:delay=int(rm[1])*160
            elif slide_number==1 and group==1:delay=min(idx,3)*180
            eff=tn(el(bl,'par'),presetID='22' if filt.startswith('wipe') else '10',presetClass='entr',presetSubtype='1' if filt.startswith('wipe') else '0',fill='hold',grpId='0',nodeType='clickEffect' if idx==0 else 'withEffect')
            condition(eff,str(delay));ec=el(eff,'childTnLst')
            vis=el(ec,'set');beh=el(vis,'cBhvr');vt=tn(beh,dur='1',fill='hold');condition(vt)
            target=el(beh,'tgtEl');el(target,'spTgt',spid=sid);names=el(beh,'attrNameLst');el(names,'attrName').text='style.visibility'
            to=el(vis,'to');el(to,'strVal',val='visible')
            ae=el(ec,'animEffect',transition='in',filter=filt);ab=el(ae,'cBhvr');tn(ab,dur='350' if filt.startswith('wipe') else '300');target=el(ab,'tgtEl');el(target,'spTgt',spid=sid)
            entries.append({'beat':group,'shape':sid,'name':name,'effect':filt,'delay_ms':delay})
    for tag,event in [('prevCondLst','onPrev'),('nextCondLst','onNext')]:
        l=el(seq,tag);c=el(l,'cond',evt=event,delay='0');el(el(c,'tgtEl'),'sldTgt')
    assert sorted(groups)==list(range(1,max(groups)+1))
    ids=[n.get('id') for n in root.findall('.//p:cNvPr',NS)]
    assert len(ids)==len(set(ids)), 'duplicate shape id'
    tids=[n.get('id') for n in timing.findall('.//p:cTn',NS)]
    assert len(tids)==len(set(tids)), 'duplicate timing id'
    assert all(n.get('spid') in ids for n in timing.findall('.//p:spTgt',NS))
    assert not timing.findall('.//p:animEffect[@transition="out"]',NS)
    return {'slide':slide_number,'clicks':len(groups),'animated_objects':len(entries),'transition':'Fade','effects':entries}

with zipfile.ZipFile(ROOT/'draft.pptx') as z:files={n:z.read(n) for n in z.namelist()}
report=[]
for i in range(1,13):
    key=f'ppt/slides/slide{i}.xml';r=E.fromstring(files[key])
    if i in (10,11):table_rows(r,4 if i==10 else 1)
    report.append(animate(r,i));files[key]=E.tostring(r,xml_declaration=True,encoding='UTF-8',standalone=True)
with zipfile.ZipFile(ROOT/'animated.pptx','w',zipfile.ZIP_DEFLATED) as z:
    for n,v in files.items():z.writestr(n,v)
(ROOT/'animation-audit.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print(json.dumps([{'slide':r['slide'],'clicks':r['clicks'],'objects':r['animated_objects']} for r in report]))
