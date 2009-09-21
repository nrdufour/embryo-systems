-module(adtm_family).
-behavior(gen_server).

-export([execute/3, start_link/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2,
	terminate/2, code_change/3]).

start_link() ->
	gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).

execute(Operation, Names, Extra) ->
	gen_server:call(?MODULE, {execute, {Operation, Names, Extra}}).

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

execute_operation({Operation, Names, _Extra}) ->
	io:format("Familiy Manager: Executing ~p operation on ~p~n", [Operation, Names]).

