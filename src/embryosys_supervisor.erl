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

-module(embryosys_supervisor).
-behavior(supervisor).

-export([start/0, start_in_shell_for_testing/0, start_link/1, init/1]).

start() ->
	spawn(fun() ->
		supervisor:start_link({local, ?MODULE}, ?MODULE, _Arg = [])
	end).

start_in_shell_for_testing() ->
	{ok, Pid} = supervisor:start_link({local, ?MODULE}, ?MODULE, _Arg = []),
	unlink(Pid).

start_link(Args) ->
	supervisor:start_link({local, ?MODULE}, ?MODULE, Args).

init([]) ->
	%% Install my personal error handler
	%%gen_event:swap_handler(alarm_handler,
	%%			{alarm_handler, swap},
	%%			{my_alarm_handler, xyz}),
	
	{ok, {{one_for_one, 3, 10},
		[{tag1,
			{adt_server, start_link, []},
			permanent,
			10000,
			worker,
			[adt_server]},
		{tag2,
			{adtm_family, start_link, []},
			permanent,
			10000,
			worker,
			[adtm_family]},
		{tag3,
			{adtm_property, start_link, []},
			permanent,
			10000,
			worker,
			[adtm_property]},
		{tag4,
			{adtm_link, start_link, []},
			permanent,
			10000,
			worker,
			[adtm_link]},
		{tag5,
			{adtm_entity, start_link, []},
			permanent,
			10000,
			worker,
			[adtm_entity]},
		{tag6,
			{storage_server, start_link, []},
			permanent,
			10000,
			worker,
			[storage_server]}
	]}}.

