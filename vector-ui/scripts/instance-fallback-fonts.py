"""Build fixed-weight portable fonts without subsetting glyphs (fonttools==4.68.0)."""
from pathlib import Path
import hashlib,json,argparse
from fontTools.ttLib.scaleUpem import scale_upem
import fontTools
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
assert fontTools.__version__=='4.68.0', 'Use fonttools==4.68.0 for reproducible font generation'
root=Path(__file__).resolve().parents[1]/'assets/fallback'
parser=argparse.ArgumentParser()
parser.add_argument('--output-dir',type=Path)
parser.add_argument('--cjk-upem',type=int,default=1000)
args=parser.parse_args()
assert 16<=args.cjk_upem<=16384
output_root=args.output_dir or root
assert args.cjk_upem==1000 or args.output_dir, 'Experimental UPEM requires a separate output directory'
output_root.mkdir(parents=True,exist_ok=True)
provenance=json.loads((root/'PROVENANCE.json').read_text())
report={'fonttools':fontTools.__version__,'weight':400,'subsetting':False,'sources_revision':provenance['revision'],'fonts':[]}
def canonical_cmaps(font):
    return sorted(json.dumps({'platform':t.platformID,'encoding':t.platEncID,'format':t.format,'language':getattr(t,'language',0),'cmap':getattr(t,'cmap',{}),'uvs':getattr(t,'uvsDict',{})},sort_keys=True) for t in font['cmap'].tables)
for source,name in [('NotoSansSC.ttf','FormaCJK'),('NotoEmoji.ttf','FormaEmoji')]:
    data=(root/source).read_bytes();expected=next(f['sha256'] for f in provenance['files'] if f['path']==source)
    assert hashlib.sha256(data).hexdigest()==expected
    font=TTFont(root/source,recalcTimestamp=False)
    cmap=font.getBestCmap().copy();order=font.getGlyphOrder()[:];all_cmaps=canonical_cmaps(font)
    reference_advances={}
    original_upem=font['head'].unitsPerEm
    if source=='NotoSansSC.ttf' and font['head'].unitsPerEm!=args.cjk_upem:
        assert args.cjk_upem % original_upem == 0, 'Use an integer UPEM multiplier to preserve metrics exactly'
        reference=instantiateVariableFont(TTFont(root/source,recalcTimestamp=False),{'wght':400},inplace=True,optimize=True,static=True)
        for table in ['hmtx','vmtx']:
            if table in reference:
                reference_advances[table]={glyph:metrics[0] for glyph,metrics in reference[table].metrics.items()}
        scale_upem(font,args.cjk_upem)
    instance=instantiateVariableFont(font,{'wght':400},inplace=True,optimize=True,static=True)
    for table,advances in reference_advances.items():
        multiplier=args.cjk_upem//original_upem
        for glyph,advance in advances.items():
            _,bearing=instance[table].metrics[glyph]
            instance[table].metrics[glyph]=(advance*multiplier,bearing)
    assert 'fvar' not in instance and 'gvar' not in instance
    assert instance.getBestCmap()==cmap and instance.getGlyphOrder()==order
    names={1:name,2:'Regular',3:name+'-Regular-weight400',4:name+' Regular',6:name+'-Regular',16:name,17:'Regular',21:name,22:'Regular',25:name}
    for record in instance['name'].names:
        if record.nameID in names:record.string=names[record.nameID].encode(record.getEncoding())
    output=output_root/(name+'-Regular.ttf');instance.recalcTimestamp=False;instance.save(output)
    saved=TTFont(output,recalcTimestamp=False);assert saved.getBestCmap()==cmap and saved.getGlyphOrder()==order
    assert canonical_cmaps(saved)==all_cmaps
    for table,advances in reference_advances.items():
        assert all(saved[table].metrics[glyph][0] == advance*(args.cjk_upem//original_upem) for glyph,advance in advances.items())
    entry={'source':source,'source_sha256':expected,'output':output.name,'output_sha256':hashlib.sha256(output.read_bytes()).hexdigest(),'source_bytes':len(data),'output_bytes':output.stat().st_size,'unicode_mappings':len(cmap),'glyphs':len(order),'units_per_em':saved['head'].unitsPerEm,'reference_advance_tables':sorted(reference_advances),'glyph_order_preserved':True,'best_cmap_preserved':True,'all_cmap_tables_preserved':True}
    report['fonts'].append(entry);print(json.dumps(entry),flush=True)
(output_root/'STATIC_INSTANCES.json').write_text(json.dumps(report,indent=2)+'\n')
