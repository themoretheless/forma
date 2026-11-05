//! Offline Unicode17 word oracle; UI whitespace/grapheme policy tested separately.
use forma::control_state::TextEdit;
use unicode_segmentation::UnicodeSegmentation;
fn corpus()->Vec<(usize,String,Vec<usize>)>{
    let source=include_str!("fixtures/unicode-17.0.0/WordBreakTest.txt");
    assert!(source.starts_with("# WordBreakTest-17.0.0.txt"));
    assert_eq!(unicode_segmentation::UNICODE_VERSION,(17,0,0));
    let cases:Vec<_>=source.lines().enumerate().filter_map(|(line,text)|{
        let data=text.split('#').next().unwrap().trim();if data.is_empty(){return None;}
        let(mut value,mut bounds)=(String::new(),Vec::new());
        for token in data.split_whitespace(){match token{"÷"=>bounds.push(value.len()),"×"=>{},code=>value.push(char::from_u32(u32::from_str_radix(code,16).unwrap()).unwrap())}}
        assert_eq!(bounds.first(),Some(&0));assert_eq!(bounds.last(),Some(&value.len()));
        Some((line+1,value,bounds))
    }).collect();assert_eq!(cases.len(),1944);cases
}
#[test]
fn unicode17_full_word_boundary_corpus(){
    for(line,value,expected)in corpus(){
        let mut actual:Vec<_>=value.split_word_bound_indices().map(|(at,_)|at).collect();actual.push(value.len());
        assert_eq!(actual,expected,"word boundary line{line}");
    }
}
#[test]
fn unicode17_editor_word_actions_follow_reference_segments(){
    for(line,value,bounds)in corpus(){
        let segments:Vec<_>=bounds.windows(2).filter(|pair|!value[pair[0]..pair[1]].chars().all(char::is_whitespace)).collect();
        // Grapheme rounding is a separate UI policy, qualified by GraphemeBreakTest.
        let mut clusters:Vec<_>=value.grapheme_indices(true).map(|(at,_)|at).collect();clusters.push(value.len());
        let floor=|at|*clusters.iter().rev().find(|&&b|b<=at).unwrap();
        let ceil=|at|*clusters.iter().find(|&&b|b>=at).unwrap();
        for at in value.char_indices().map(|(at,_)|at).chain(std::iter::once(value.len())){
            for forward in [false,true]{
                let target=if forward{ceil(segments.iter().find(|p|p[1]>at).map_or(value.len(),|p|p[1]))}else{floor(segments.iter().rev().find(|p|p[0]<at).map_or(0,|p|p[0]))};
                let mut edit=TextEdit::with_multiline(value.clone(),true).unwrap();edit.set_selection(at,at).unwrap();
                edit.move_words(if forward{1}else{-1},true);assert_eq!(edit.caret(),target,"move line{line} byte{at} forward{forward}");
                edit.set_selection(at,at).unwrap();
                let range=if forward{floor(at)..target}else{target..ceil(at)};
                let expected=format!("{}{}",&value[..range.start],&value[range.end..]);
                if forward{edit.delete_word();}else{edit.backspace_word();}
                assert_eq!(edit.value(),expected,"delete line{line} byte{at} forward{forward}");
                if !range.is_empty(){assert!(edit.undo());assert_eq!(edit.value(),value);assert_eq!(edit.caret(),at);assert!(edit.redo());assert_eq!(edit.value(),expected);}
            }
        }
    }
}
