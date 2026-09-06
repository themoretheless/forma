use tao::{event::{Event,WindowEvent},event_loop::{ControlFlow,EventLoopBuilder},window::WindowBuilder};
use wry::WebViewBuilder;

// The runner supplies the current project's actions.rs at compilation time.
mod actions { include!(env!("FORMA_ACTIONS")); }

fn main() -> Result<(),Box<dyn std::error::Error>> {
    let filename=std::env::args().nth(1).ok_or("Expected UI snapshot path")?;
    let snapshot:serde_json::Value=serde_json::from_str(&std::fs::read_to_string(filename)?)?;
    let html=snapshot["html"].as_str().ok_or("Missing HTML")?;
    // Runtime defaults belong to Rust, never to a designer scenario.
    let mut state=actions::SearchState {
        query:String::new(),
        loading:false,
        status:String::from("Готов к поиску"),
    };
    let mut events=EventLoopBuilder::<String>::with_user_event().build();
    #[cfg(target_os="macos")]
    {
        use tao::platform::macos::{EventLoopExtMacOS,ActivationPolicy};
        events.set_activation_policy(ActivationPolicy::Regular);
        events.set_dock_visibility(true);
        events.set_activate_ignoring_other_apps(true);
    }
    let proxy=events.create_proxy();
    let window=WindowBuilder::new().with_title("Forma — приложение").with_inner_size(tao::dpi::LogicalSize::new(760.,820.)).build(&events)?;
    let builder=WebViewBuilder::new().with_html(html)
        .with_navigation_handler(|url|url=="about:blank")
        .with_ipc_handler(move |request|{let _=proxy.send_event(request.body().clone());});
    #[cfg(any(target_os="macos",target_os="windows"))]
    let webview=builder.build(&window)?;
    #[cfg(target_os="linux")]
    let webview={use tao::platform::unix::WindowExtUnix;use wry::WebViewBuilderExtUnix;builder.build_gtk(window.default_vbox().unwrap())?};
    println!("Native window opened");
    events.run(move |event,_,flow| {
        *flow=ControlFlow::Wait;
        match event {
            Event::NewEvents(tao::event::StartCause::Init)=>{
                window.set_visible(true);
                window.set_focus();
            }
            Event::WindowEvent{event:WindowEvent::CloseRequested,..}=>*flow=ControlFlow::Exit,
            Event::UserEvent(message)=>{
                if let Ok(value)=serde_json::from_str::<serde_json::Value>(&message){
                    match value["kind"].as_str(){
                        Some("query")=>state.query=value["value"].as_str().unwrap_or("").to_string(),
                        Some("search")=>{actions::search(&mut state);println!("actions::search query={}",state.query);},
                        _=>{}
                    }
                    let value=serde_json::json!({"query":state.query,"loading":state.loading,"status":state.status});
                    let _=webview.evaluate_script(&format!("window.applyState({});",value));
                }
            }
            _=>{}
        }
    });
}
