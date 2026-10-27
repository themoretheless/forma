// A native window has no DOM to float regions over, so the same studio-ui regions are composed
// into one scene: the top bar, the canvas area, the corner and the palette as a modal layer.
// Each region keeps its own file; only its root container is lifted into the window.

// The bundled font has no ⌘/↵ glyphs, so shortcut hints are spelled out.
export const keyText=key=>(key??'').replace('⌘','Cmd ').replace('↵','Enter').trim();

// The root block of `component Name { … }`: everything between the component's braces.
function body(source,path){
 const open=source.indexOf('{',source.search(/\bcomponent\s+\w+/));
 if(open<0)throw Error(`${path}: нет компонента`);
 let depth=0,quote=null;
 for(let i=open;i<source.length;i++){
  const c=source[i];
  if(quote){if(c==='\\')i++;else if(c===quote)quote=null;continue;}
  if(c==="'"||c==='"'){quote=c;continue;}
  if(c==='/'&&source[i+1]==='/'){i=source.indexOf('\n',i);if(i<0)break;continue;}
  if(c==='{')depth++;
  else if(c==='}'&&--depth===0)return source.slice(open+1,i).trim();
 }
 throw Error(`${path}: незакрытый компонент`);
}
// Places a region's root container in a grid cell of the window.
const inCell=(block,cell)=>block.replace(/^(\w+\s*\{)/,`$1 cell: ${cell};`);

export function composeStudioWindow(files,{width,height,paletteHeight}){
 const region=name=>body(files[`ui/${name}.ui`],`ui/${name}.ui`);
 // A modal layer paints nothing itself, so the palette column sits on a surface in a stack.
 const palette=body(files['ui/Palette.ui'],'ui/Palette.ui');
 return `component StudioWindow {
    Column {
        width: ${width};
        height: ${height};
        background: state.theme.canvas;
        Grid {
            width: *;
            height: 48;
            columns: [-, *, -, *, -];
            ${inCell(region('Title'),'1 1')}
            ${inCell(region('States'),'1 3')}
            ${inCell(region('Run'),'1 5')}
        }
        Surface {
            key: 'canvas';
            width: *;
            height: *;
            borderWidth: 0;
            revealWidth: 0;
            background: #00000000;
            color: state.theme.textMuted;
            text: state.canvasText;
        }
        Grid {
            width: *;
            height: 44;
            columns: [*, -];
            ${inCell(region('Corner'),'1 2')}
        }
        Modal {
            key: 'palette';
            open: state.paletteOpen;
            y: 72;
            width: 520;
            height: ${paletteHeight};
            backdrop: #00000066;
            dismissed -> actions.closePalette();
            Stack {
                width: 520;
                height: ${paletteHeight};
                Surface { key: 'paletteSurface'; width: *; height: *; radius: 12; revealWidth: 0; background: state.theme.surfaceRaised; }
                ${palette}
            }
        }
    }
}
`;
}
