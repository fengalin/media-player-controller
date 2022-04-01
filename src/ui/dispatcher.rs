use super::{app, App};

pub struct Dispatcher<T>(std::marker::PhantomData<*const T>);

impl Dispatcher<super::ControlSurfacePanel> {
    pub fn handle(app: &mut App, resp: Option<super::ctrl_surf::Response>) {
        if let Some(resp) = resp {
            use super::ctrl_surf::Response::*;

            app.clear_last_err();

            match resp {
                Use(ctrl_surf) => {
                    app.send_req(app::Request::UseControlSurface(ctrl_surf));
                }
                NoControlSurface => {
                    app.send_req(app::Request::NoControlSurface);
                }
                Discover => {
                    todo!();
                }
            }
        }
    }
}

impl Dispatcher<super::PortsPanel> {
    pub fn handle(app: &mut App, resp: Option<super::port::Response>) {
        if let Some(resp) = resp {
            use super::port::Response::*;

            app.clear_last_err();
            app.send_req(app::Request::RefreshPorts);

            match resp {
                Connect((direction, port_name)) => {
                    app.send_req(app::Request::Connect((direction, port_name)));
                }
                Disconnect(direction) => {
                    app.send_req(app::Request::Disconnect(direction));
                }
                CheckingList => (), // only refresh ports & clear last_err
            }
        }
    }
}

impl Dispatcher<super::PlayerPanel> {
    pub fn handle(app: &mut App, resp: Option<super::mpris::Response>) {
        if let Some(resp) = resp {
            use super::mpris::Response::*;

            app.clear_last_err();
            app.send_req(app::Request::RefreshPlayers);

            match resp {
                Use(player_name) => {
                    app.send_req(app::Request::UsePlayer(player_name));
                }
                CheckingList => (), // only refresh ports & clear last_err
            }
        }
    }
}
