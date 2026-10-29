import {themes} from '../vector-ui/controls/tokens.js';
import {serializeValue} from './expressions.js';

export function shellControl({kind='button',text='',badge='',width,height,selected=false,disabled=false,checked=false,expanded=false,branch=false,showChevron=true,indent=0,icon,iconColor,tone='success'}){
 const type=({primary:'PrimaryButton',button:'SecondaryButton',ghost:'GhostButton',icon:'IconButton',accentIcon:'IconButton',leading:'LeadingIconButton',primaryLeading:'PrimaryIconButton',menu:'MenuItem',notice:'Alert',tab:'TabButton',tree:'TreeItem',check:'Checkbox',field:'TextField',select:'SelectTrigger',status:'StatusBar'})[kind];
 if(!type)throw Error(`Unknown Studio control ${kind}`);
 const props={width:Math.min(4096,Math.max(1,width)),height:Math.min(4096,Math.max(1,height)),disabled};
 if(kind==='field')Object.assign(props,{value:'',placeholder:''});
 else if(kind==='select')props.value=text;
 else if(kind==='status'){const [first,second='',third='']=text.split('\n');Object.assign(props,{first,second,third});}
 else props.text=text;
 if(['icon','accentIcon','leading','primaryLeading'].includes(kind))props.icon=icon??'assets/plus.svg';
 if(kind==='icon'&&selected)Object.assign(props,{background:{expr:themes.dark.surfaceHover},borderColor:{expr:themes.dark.primary}});
 // An icon-only primary action keeps the primary colours instead of falling back to a neutral icon.
 if(kind==='accentIcon')Object.assign(props,{background:{expr:themes.dark.primary},hoverBackground:{expr:themes.dark.primaryHover},pressedBackground:{expr:themes.dark.primaryPressed},color:{expr:themes.dark.primaryText},borderColor:{expr:themes.dark.primary}});
 if(kind==='notice')Object.assign(props,{tone,description:''});
 // A leading-icon badge in a danger state (the problems count) takes the danger ink, as its HTML twin does.
 if(kind==='leading'&&tone==='danger')props.color={expr:themes.dark.dangerInk};
 if(kind==='tab'||kind==='tree'||kind==='menu')props.selected=selected;
 if(kind==='tree'){Object.assign(props,{branch,expanded,indent});if(badge)props.badge=badge;if(!showChevron)props.showChevron=false;if(icon)props.icon=icon;if(iconColor)props.iconColor={expr:iconColor};}
 if(kind==='check')props.checked=checked;
 return {type,props};
}
export function shellScene(spec){
 return `component StudioControl { Frame { width: ${spec.props.width}; height: ${spec.props.height}; ${spec.type} { ${Object.entries(spec.props).map(([key,value])=>`${key}: ${serializeValue(value)};`).join(' ')} } } }`;
}
