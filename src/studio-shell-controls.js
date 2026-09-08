import {themes} from '../vector-ui/controls/tokens.js';
import {serializeValue} from './expressions.js';

export function shellControl({kind='button',text='',width,height,selected=false,disabled=false,checked=false,expanded=false,branch=false,indent=0,icon='assets/plus.svg',tone='success'}){
 const type=({primary:'PrimaryButton',button:'SecondaryButton',ghost:'GhostButton',icon:'IconButton',menu:'MenuItem',notice:'Alert',tab:'TabButton',tree:'TreeItem',check:'Checkbox',field:'TextField',select:'SelectTrigger',status:'StatusBar'})[kind];
 if(!type)throw Error(`Unknown Studio control ${kind}`);
 const props={width:Math.min(4096,Math.max(1,width)),height:Math.min(4096,Math.max(1,height)),disabled};
 if(kind==='field')Object.assign(props,{value:'',placeholder:''});
 else if(kind==='select')props.value=text;
 else if(kind==='status'){const [first,second='',third='']=text.split('\n');Object.assign(props,{first,second,third});}
 else props.text=text;
 if(kind==='icon'){props.icon=icon;if(selected)Object.assign(props,{background:{expr:themes.dark.surfaceHover},borderColor:{expr:themes.dark.primary}});}
 if(kind==='notice')Object.assign(props,{tone,description:''});
 if(kind==='tab'||kind==='tree'||kind==='menu')props.selected=selected;
 if(kind==='tree')Object.assign(props,{branch,expanded,indent});
 if(kind==='check')props.checked=checked;
 return {type,props};
}
export function shellScene(spec){
 return `component StudioControl { Frame { width: ${spec.props.width}; height: ${spec.props.height}; ${spec.type} { ${Object.entries(spec.props).map(([key,value])=>`${key}: ${serializeValue(value)};`).join(' ')} } } }`;
}
