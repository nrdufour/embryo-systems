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
-author('Nicolas R Dufour <nrdufour@gmail.com>').

-include("adt.hrl").

-export([execute/2]).

execute(Operation, Name) ->
	case Operation of
		create -> create(Name);
		hibern -> hadr(Operation, Name);
		awake -> hadr(Operation, Name);
		destroy -> hadr(Operation, Name);
		resur -> hadr(Operation, Name);
		purge -> purge(Name)
	end.

create(Name) ->
	io:format("Create family ~p~n", [Name]),
	Adt = adtm:new(family, Name),
	storage_server:store(family, Name, Adt),
	ok.

hadr(Operation, Name) ->
	io:format("~p family ~p~n", [Operation, Name]),
	Previous = storage_server:load(family, Name),
	PreviousState = Previous#adt.state,
	NewState = adtm:new_state_after(Operation, PreviousState),
	Adt = Previous#adt{state = NewState},
	storage_server:store(family, Name, Adt),
	ok.

purge(Name) ->
	io:format("Purge family ~p~n", [Name]),
	ok.

