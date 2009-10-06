%% @author Nicolas R Dufour <nrdufour@gmail.com>
%% @copyright 2009 Nicolas R Dufour.
%%
%%
%% Licensed under the Apache License, Version 2.0 (the "License");
%% you may not use this file except in compliance with the License.
%% You may obtain a copy of the License at
%%
%%     http://www.apache.org/licenses/LICENSE-2.0
%%
%% Unless required by applicable law or agreed to in writing, software
%% distributed under the License is distributed on an "AS IS" BASIS,
%% WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
%% See the License for the specific language governing permissions and
%% limitations under the License.

-module(storage_server).
-behavior(gen_server).

-export([store/3, load/2, start_link/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2,
	terminate/2, code_change/3]).

start_link() ->
	gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).

store(Type, Id, Data) ->
	gen_server:call(?MODULE, {store, {{Type, Id}, Data}}).

load(Type, Id) ->
	gen_server:call(?MODULE, {load, {Type, Id}}).

init([]) ->
	process_flag(trap_exit, true),
	io:format("~p starting~n", [?MODULE]),

	%% opening dets files
	%% FIXME need a nice way to configure the path of those files
	dets:open_file(embryosys.dets, [{type, set}]),

	{ok, []}.

handle_call({store, {Header, Data}}, _From, State) ->
	Reply = do_store(Header, Data),
	{reply, Reply, State};

handle_call({load, Header}, _From, State) ->
	Reply = do_load(Header),
	{reply, Reply, State}.

handle_cast(_Msg, State) -> {noreply, State}.

handle_info(_Info, State) -> {noreply, State}.

terminate(_Reason, _State) ->
	io:format("~p stopping~n", [?MODULE]),

	%% closing the files
	%% FIXME still need a way to configure the path
	dets:close(embryosys.dets),

	ok.

code_change(_OldVsn, State, _Extra) -> {ok, State}.

%%% ==========================================================================

do_store(Header, Data) ->
	dets:insert(embryosys.dets, {Header, Data}),
	ok.

do_load(Header) ->
	case dets:lookup(embryosys.dets, Header) of
		[{_ReturnedHeader, Data}] -> Data;
		_ -> not_found
	end.

%%
