-module(embryosys_app).
-behavior(application).

-export([start/2, stop/1]).

start(_Type, StartArgs) ->
	embryosys_supervisor:start_link(StartArgs).

stop(_State) ->
	ok.

