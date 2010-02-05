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

-module(embryosys_adtm).
-author('Nicolas R Dufour <nrdufour@gmail.com>').

-include("adt.hrl").

%% API exports
-export([do_create/2, do_hadr/3, do_purge/2]).

is_parent_classes_alive(Parents) ->
	lists:foldl(
		fun(ClassName, Previous) ->
			ClassAdt = embryosys_storage_server:load(class, ClassName),
			ClassIsValid = ClassAdt#class.state =:= alive,
			Previous and ClassIsValid
		end, true, Parents).

is_adt_valid_for_operation(Operation, Type, ElementName) ->
	Adt = embryosys_storage_server:load(Type, ElementName),

	CurrentState = if
		Adt =:= not_found -> none;
		true              -> embryosys_util:get_adt_state(Type, Adt)
	end,
		
	NextState = embryosys_util:new_state_after(Operation, CurrentState),

	{Adt, NextState}.

do_create(Type, Names) ->
	ElementName = lists:last(Names),

	{_Adt, NextState} = is_adt_valid_for_operation(create, Type, ElementName),

	if
		NextState =/= wrong_state ->
			AliveClass = #class{ name = ElementName, state = alive },
			embryosys_storage_server:store(class, ElementName, AliveClass),
			{ok, AliveClass};
		true ->
			{already_created, []}
	end.

do_hadr(Operation, Type, Names) ->
	ElementName = lists:last(Names),

	{Adt, NextState} = is_adt_valid_for_operation(Operation, Type, ElementName),

	if
		NextState =/= wrong_state ->
			UpdatedAdt = embryosys_util:set_adt_state(Type, Adt, NextState),
			embryosys_storage_server:store(class, ElementName, UpdatedAdt),
			{ok, UpdatedAdt};
		true ->
			{wrong_state, []}
	end.

do_purge(_Type, _Names) ->
	{not_yet_implemented, []}.

