%% @author Nicolas R Dufour <nrdufour@gmail.com>
%% @copyright 2009-2010 Nicolas R Dufour.
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

-module(embryosys_adt).
-author('Nicolas R Dufour <nrdufour@gmail.com>').

-export([new_state_after/2, is_ready_for/2, get_adt_state/2, set_adt_state/3]).

-include("adt.hrl").

%% State transition grid (strict)
new_state_after(Operation, State) ->
	case {Operation, State} of
		{create, none}   -> alive;
		{hibern, alive}    -> frozen;
		{awake, frozen}    -> alive;
		{destroy, alive}   -> destroyed;
		{resur, destroyed} -> alive;
		{purge, destroyed} -> none;
		{_, _}             -> wrong_state
	end.

is_ready_for(Operation, State) ->
	new_state_after(Operation, State) /= wrong_state.

get_adt_state(class, Adt) ->
	Adt#class.state;
get_adt_state(attribute, Adt) ->
	Adt#attribute.state;
get_adt_state(link, Adt) ->
	Adt#link.state;
get_adt_state(object, Adt) ->
	Adt#object.state;
get_adt_state(_, _) ->
	throw(invalid_type).

set_adt_state(class, Adt, NewState) ->
	Adt#class{state = NewState};
set_adt_state(attribute, Adt, NewState) ->
	Adt#attribute{state = NewState};
set_adt_state(link, Adt, NewState) ->
	Adt#link{state = NewState};
set_adt_state(object, Adt, NewState) ->
	Adt#object{state = NewState};
set_adt_state(_, _, _) ->
	throw(invalid_type).

%%
