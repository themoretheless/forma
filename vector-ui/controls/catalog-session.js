// Demonstration host for the library's Rust state models. These models are also
// public native Rust types; application bindings are not part of this adapter.
export function createCatalogSession(runtime, onChange = () => {}) {
  const owned=[];
  const own=model=>(owned.push(model),model);
  const check=own(new runtime.CheckValue(1,false)), mixed=own(new runtime.CheckValue(2,true));
  const sw=own(new runtime.ToggleValue(true));
  const chips=[true,false,true].map(value=>own(new runtime.ToggleValue(value)));
  const formatting=Array.from({length:5},()=>own(new runtime.ToggleValue(false)));
  const radio=own(new runtime.SelectionValue(3,1)), segment=own(new runtime.SelectionValue(3,0));
  const underline=own(new runtime.SelectionValue(3,0)), side=own(new runtime.SelectionValue(3,1));
  const page=own(new runtime.SelectionValue(5,1)), target=own(new runtime.SelectionValue(3,0));
  const range=own(new runtime.RangeValue(1,16,1,8));
  const sidebar=own(new runtime.RangeValue(70,136,1,90));
  let splitterAnchor=0;
  const tree=own(new runtime.TreeValue(new Int32Array([-1,0,0,2,2]),new Uint32Array([1,0,1,0,0])));
  tree.toggle(0); tree.toggle(2); tree.select(3);
  let documents=['ui.rs','lib.rs','main.rs'], documentIndex=0;
  let menu=false, disclosure=false, dialog=false, toast=false, tooltip=false, toastTimer=null, destroyed=false;
  const groups={protocol:[radio,'radiochips'],segment:[segment,'segments'],underline:[underline,'segments'],side:[side,'navigation'],page:[page,'navigation'],target:[target,'overlays']};
  const changed=(section,focus)=>{if(!destroyed)onChange(section,focus);};
  function state() {
    return {check:check.state()===1,mixed:mixed.state(),switch:sw.value(),chips:chips.map(c=>c.value()),
      radio:radio.selected(),segment:segment.selected(),underline:underline.selected(),side:side.selected(),page:page.selected(),target:target.selected(),range:range.value(),
      menu,disclosure,dialog,toast,tooltip,sidebar:sidebar.value(),documents:[...documents],document:documentIndex,formatting:formatting.map(v=>v.value()),
      treeSelected:tree.selected(),treeRows:Array.from(tree.visible(),index=>({index,depth:[0,1,1,2,2][index],branch:index===0||index===2,expanded:tree.expanded(index)}))};
  }
  function dispatch(action) {
    const key=action.replace(/^actions\./,'');
    if(key==='check'){check.activate();changed('choices','check');return true;}
    if(key==='mixed'){mixed.activate();changed('choices','mixed');return true;}
    if(key==='switch'){sw.toggle();changed('choices','switch');return true;}
    const chip=/^chip(\d)$/.exec(key);
    if(chip&&chips[+chip[1]]){chips[+chip[1]].toggle();changed('radiochips',key);return true;}
    const format=/^format(\d)$/.exec(key);
    if(format&&formatting[+format[1]]){formatting[+format[1]].toggle();changed('formatting',key);return true;}
    for(const [prefix,[model,section]] of Object.entries(groups)) {
      if(!new RegExp(`^${prefix}[0-9]+$`).test(key))continue;
      model.select(Number(key.slice(prefix.length)));
      if(prefix==='target')menu=false;
      changed(section,prefix==='target'?'selectToggle':key);return true;
    }
    if(key==='rangeLess'||key==='rangeMore'){range.step_by(key==='rangeLess'?-1:1);changed('range',key);return true;}
    if(key==='pagePrev'||key==='pageNext'){page.next(key==='pagePrev'?-1:1,false);changed('navigation',key);return true;}
    if(key==='selectToggle'){menu=!menu;changed('overlays',menu?`target${target.selected()}`:'selectToggle');return true;}
    if(key==='disclosure'){disclosure=!disclosure;changed('overlays',key);return true;}
    if(key==='showDialog'){dialog=true;changed('feedback','demoDialogClose');return true;}
    if(key==='demoDialogClose'||key==='demoDialogConfirm'){dialog=false;changed('feedback','showDialog');return true;}
    if(key==='showTooltip'){tooltip=!tooltip;changed('feedback','showTooltip');return true;}
    if(key==='showToast'){
      toast=true;clearTimeout(toastTimer);changed('feedback','showToast');
      toastTimer=setTimeout(()=>{toast=false;changed('feedback');},2600);return true;
    }
    const doc=/^doc(\d+)$/.exec(key), close=/^docClose(\d+)$/.exec(key);
    if(doc){documentIndex=+doc[1];changed('documents',key);return true;}
    if(close){const index=+close[1];documents.splice(index,1);documentIndex=Math.max(0,Math.min(documentIndex-(index<documentIndex?1:0),documents.length-1));changed('documents',documents.length?`doc${documentIndex}`:'resetDocuments');return true;}
    if(key==='resetDocuments'){documents=['ui.rs','lib.rs','main.rs'];documentIndex=0;changed('documents','doc0');return true;}
    const row=/^tree(\d+)$/.exec(key);
    if(row){const index=+row[1];tree.select(index);if(index===0||index===2)tree.toggle(index);changed('tree',key);return true;}
    return false;
  }
  function pointer(phase,{node,x,width}) {
    if(node?.props.key==='appSplitter') {
      if(phase==='start')splitterAnchor=x;
      else sidebar.set(sidebar.value()+x-splitterAnchor);
      changed('shell',phase==='end'?'appSplitter':undefined);return true;
    }
    if(node?.props.key!=='rangeSlider')return false;
    range.set_fraction(Math.max(0,Math.min(1,(x-8)/(width-16))));
    changed('range',phase==='end'?'rangeSlider':undefined);return true;
  }
  function key(event,node) {
    const id=node?.props.key;
    if(!id)return false;
    if(id==='appSplitter'&&['ArrowLeft','ArrowRight','Home','End'].includes(event.key)) {
      if(event.key==='Home')sidebar.set(70);else if(event.key==='End')sidebar.set(136);else sidebar.step_by(event.key==='ArrowLeft'?-4:4);
      changed('shell',id);return true;
    }
    if(event.key==='Escape') {
      if(dialog){dialog=false;changed('feedback','showDialog');return true;}
      if(menu){menu=false;changed('overlays','selectToggle');return true;}
      if(tooltip){tooltip=false;changed('feedback','showTooltip');return true;}
    }
    if(dialog&&id.startsWith('demoDialog')&&event.key==='Tab') {
      const order=['demoDialogClose','demoDialogCancel','demoDialogConfirm'];
      changed('feedback',order[(order.indexOf(id)+(event.shiftKey?-1:1)+order.length)%order.length]);return true;
    }
    if(id==='rangeSlider'&&['ArrowLeft','ArrowRight','ArrowDown','ArrowUp','Home','End','PageUp','PageDown'].includes(event.key)) {
      if(event.key==='Home')range.set(1);else if(event.key==='End')range.set(16);
      else range.step_by(({ArrowLeft:-1,ArrowRight:1,ArrowDown:-1,ArrowUp:1,PageUp:5,PageDown:-5})[event.key]);
      changed('range',id);return true;
    }
    if(/^tree\d+$/.test(id)&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)) {
      tree.select(Number(id.slice(4)));tree[({ArrowLeft:'left',ArrowRight:'right',ArrowUp:'up',ArrowDown:'down'})[event.key]]();
      changed('tree',`tree${tree.selected()}`);return true;
    }
    for(const [prefix,[model,section]] of Object.entries(groups)) {
      if(new RegExp(`^${prefix}[0-9]+$`).test(id)&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home','End'].includes(event.key)) {
        if(event.key==='Home')model.select(0);else if(event.key==='End')model.select(prefix==='page'?4:2);
        else model.next(['ArrowLeft','ArrowUp'].includes(event.key)?-1:1,true);
        changed(section,`${prefix}${model.selected()}`);return true;
      }
    }
    return false;
  }
  return {state,dispatch,pointer,key,destroy(){destroyed=true;clearTimeout(toastTimer);for(const model of owned)model.free();}};
}
