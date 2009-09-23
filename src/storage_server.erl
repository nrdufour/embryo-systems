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

-export([store/2, start_link/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2,
	terminate/2, code_change/3]).

start_link() ->
	gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).

init([]) ->
	%% Note we must set trap_exit = true if we
	%% want terminate/2 to be called when the application
	%% is stopped
	process_flag(trap_exit, true),
	io:format("~p starting~n", [?MODULE]),

	%% opening dets files
	%% FIXME need a nice way to configure the path of those files
	dets:open_file(es_family.dets, [{type, set}]),
	dets:open_file(es_property.dets, [{type, set}]),
	dets:open_file(es_link.dets, [{type, set}]),
	dets:open_file(es_entity.dets, [{type, set}]),

	{ok, 0}.

handle_call({execute, Args}, _From, N) -> {reply, ok, N+1}.

handle_cast(_Msg, N) -> {noreply, N}.

handle_info(_Info, N) -> {noreply, N}.

terminate(_Reason, _N) ->
	io:format("~p stopping~n", [?MODULE]),

	%% closing the files
	%% FIXME still need a way to configure the path

	dets:close(es_family.dets),
	dets:close(es_property.dets),
	dets:close(es_link.dets),
	dets:close(es_entity.dets),

	ok.

code_change(_OldVsn, N, _Extra) -> {ok, N}.

%%% ==========================================================================

store(family, Data) ->
	io:format("Storing data ~p in family file~n", [Data]),
	dets:insert(es_family.dets, Data);

store(property, Data) ->
	io:format("Storing data ~p in property file~n", [Data]),
	dets:insert(es_property.dets, Data);

store(link, Data) ->
	io:format("Storing data ~p in link file~n", [Data]),
	dets:insert(es_link.dets, Data);

store(entity, Data) ->
	io:format("Storing data ~p in entity file~n", [Data]),
	dets:insert(es_entity.dets, Data).

%%
