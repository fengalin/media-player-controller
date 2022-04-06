use super::{app::Request, ctrl_surf, player, port, App};

pub struct Dispatcher<T>(std::marker::PhantomData<*const T>);

impl Dispatcher<super::ControlSurfacePanel> {
    pub fn handle(app: &mut App, resp: Option<ctrl_surf::Response>) {
        if let Some(resp) = resp {
            use ctrl_surf::Response::*;

            app.clear_last_err();

            match resp {
                Use(ctrl_surf) => {
                    app.send_req(Request::UseControlSurface(ctrl_surf));
                }
                Unuse => {
                    app.send_req(Request::NoControlSurface);
                }
                Scan => {
                    app.send_req(Request::ScanControlSurface);
                }
            }
        }
    }
}

impl Dispatcher<super::PortsPanel> {
    pub fn handle(app: &mut App, resp: Option<port::Response>) {
        if let Some(resp) = resp {
            use port::Response::*;

            app.clear_last_err();
            app.send_req(Request::RefreshPorts);

            match resp {
                Connect((direction, port_name)) => {
                    app.send_req(Request::ConnectPort((direction, port_name)));
                }
                Disconnect(direction) => {
                    app.send_req(Request::DisconnectPort(direction));
                }
                CheckingList => (), // only refresh ports & clear last_err
            }
        }
    }
}

impl Dispatcher<super::PlayerPanel> {
    pub fn handle(app: &mut App, resp: Option<player::Response>) {
        if let Some(resp) = resp {
            use player::Response::*;

            app.clear_last_err();

            if let Use(_) = resp {
                app.send_req(Request::RefreshPlayers);
            }

            app.send_req(resp.into());
        }
    }
}

impl From<player::Response> for Request {
    fn from(resp: player::Response) -> Self {
        use crate::ctrl_surf::event::Transport;
        use player::Response::*;

        match resp {
            Use(player_name) => Request::UsePlayer(player_name),
            CheckingList => Request::RefreshPlayers,
            Position(pos) => Transport::SetPosition(pos).into(),
            PlayPause => Transport::PlayPause.into(),
            Previous => Transport::Previous.into(),
            Next => Transport::Next.into(),
        }
    }
}
