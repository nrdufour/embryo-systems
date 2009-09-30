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

-module(adtm).
-author('Nicolas R Dufour <nrdufour@gmail.com>').

-export([new/2, new_id/1, new_state_after/2, is_ready_for/2]).

-include("adt.hrl").

%% create a brand new adt (blank)
new(Type, Name) ->
	#adt{id = new_id(Type), fname = Name, state = none}.

%% create an adt id baed on its type
new_id(Type) ->
	Address = case Type of
		family   -> { 0 };
		property -> { 0 , 0 };
		relation -> { 0 , 0 , 0 };
		entity   -> { 0 , 0 };
		_        -> { 0 }
	end,
	#adt_id{ type = family, address = Address }.

%% State transition grid (strict)
new_state_after(Operation, State) ->
	case {Operation, State} of
		{creation, none}   -> alive;
		{hibern, alive}    -> frozen;
		{awake, frozen}    -> alive;
		{destroy, alive}   -> destroyed;
		{resur, destroyed} -> alive;
		{purge, destroyed} -> none;
		{_, _}             -> wrong_state
	end.

is_ready_for(Operation, State) ->
	new_state_after(Operation, State) /= wrong_state.

%%
