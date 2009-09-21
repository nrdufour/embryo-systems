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

-module(adtm_family).
-behavior(gen_server).
-author('Nicolas R Dufour <nrdufour@gmail.com>').

-export([execute/2, start_link/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2,
	terminate/2, code_change/3]).

start_link() ->
	gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).

execute(Operation, Name) ->
	gen_server:call(?MODULE, {execute, {Operation, Name}}).

init([]) ->
	%% Note we must set trap_exit = true if we
	%% want terminate/2 to be called when the application
	%% is stopped
	process_flag(trap_exit, true),
	io:format("~p starting~n", [?MODULE]),
	{ok, 0}.

handle_call({execute, Args}, _From, N) -> {reply, execute_operation(Args), N+1}.

handle_cast(_Msg, N) -> {noreply, N}.

handle_info(_Info, N) -> {noreply, N}.

terminate(_Reason, _N) ->
	io:format("~p stopping~n", [?MODULE]),
	ok.

code_change(_OldVsn, N, _Extra) -> {ok, N}.

%%% ==========================================================================

execute_operation({create, Name}) ->
	io:format("Create family ~p~n", [Name]),
	ok;

execute_operation({hibern, Name}) ->
	io:format("Hibern family ~p~n", [Name]),
	ok;

execute_operation({awake, Name}) ->
	io:format("Awake family ~p~n", [Name]),
	ok;

execute_operation({destroy, Name}) ->
	io:format("Destroy family ~p~n", [Name]),
	ok;

execute_operation({resur, Name}) ->
	io:format("Resur family ~p~n", [Name]),
	ok;

execute_operation({purge, Name}) ->
	io:format("Purge family ~p~n", [Name]),
	ok.

