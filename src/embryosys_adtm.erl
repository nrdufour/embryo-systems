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

do_create(Type, Names) ->
	ElementName = lists:last(Names),

	Adt = embryosys_storage_server:load(Type, ElementName),

	%% TODO needs to take care of the other types obviously
	case Adt of
		not_found ->
			AliveClass = #class{ name = ElementName, state = alive },
			embryosys_storage_server:store(class, ElementName, AliveClass),
			{ok, AliveClass};
		_ ->
			{already_created, []}
	end.

do_hadr(Operation, Type, Names) ->
	ElementName = lists:last(Names),

	% first grab the adt from the storage
	Previous = embryosys_storage_server:load(Type, ElementName),

	case Previous of
		not_found ->
			{not_found, []};
		_ ->
			% compute its new state
			NewState = embryosys_util:new_state_after(Operation, Previous#class.state),
			case NewState of
				wrong_state ->
					{wrong_state, []};
				_ ->
					% and store it
					UpdatedAdt = Previous#class{state = NewState},
					embryosys_storage_server:store(class, ElementName, UpdatedAdt),
					{ok, UpdatedAdt}
			end
	end.

do_purge(_Type, _Names) ->
	{not_yet_implemented, []}.

