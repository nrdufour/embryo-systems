%% Copyright 2009-2010 Nicolas R Dufour.
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
%%
%% @author Nicolas R Dufour <nrdufour@gmail.com>
%% @copyright 2009-2010 Nicolas R Dufour.

-module(embryosys_adtm).
-author('Nicolas R Dufour <nrdufour@gmail.com>').
-include("adt.hrl").

%% API exports
-export([do_create/2, do_hibern/2, do_awake/2, do_destroy/2, do_resur/2, do_purge/2]).

do_create(Type, Names) ->
    do_it(create, Type, Names).

do_hibern(Type, Names) ->
    do_it(hibern, Type, Names).

do_awake(Type, Names) ->
    do_it(awake, Type, Names).

do_destroy(Type, Names) ->
    do_it(destroy, Type, Names).

do_resur(Type, Names) ->
    do_it(resur, Type, Names).

do_purge(_Type, _Names) ->
    {not_yet_implemented, []}.

%%% --------------------------------------------------------------------------

is_class_alive(ClassName) ->
    ClassAdt = embryosys_storage_server:load(class, [ClassName]),
    case ClassAdt of
        not_found -> false;
        _         -> Meta = ClassAdt#adt.meta,
		     (Meta#meta.type =:= class) and (Meta#meta.state =:= alive)
    end.

are_adt_parents_valid(class, _Names) ->
    true;
are_adt_parents_valid(Type, Names) when Type == attribute; Type == object ->
    [ClassName, _Name] = Names,
    is_class_alive(ClassName);
are_adt_parents_valid(link, Names) ->
    [FromClassName, ToClassName, _LinkName] = Names,
    is_class_alive(FromClassName) and is_class_alive(ToClassName);
are_adt_parents_valid(_Type, _Names) ->
    throw(invalid_type).

is_adt_valid_for_operation(Operation, Type, Names) ->
    AreParentsValid = are_adt_parents_valid(Type, Names),
    if AreParentsValid =:= true ->
        Adt = embryosys_storage_server:load(Type, Names),

        CurrentState = if
            Adt =:= not_found -> none;
            true              -> Meta = Adt#adt.meta,
                                 Meta#meta.state
        end,
        
        NextState = embryosys_adt:new_state_after(Operation, CurrentState),

        {Adt, NextState};
       true ->
        {none, wrong_state}
    end.

do_it(Operation, Type, Names) ->
    {Adt, NextState} = is_adt_valid_for_operation(Operation, Type, Names),

    if
        NextState =/= wrong_state ->
            UpdatedAdt = case Operation of
                create -> embryosys_adt:new_adt(Type, Names);
                _      -> Meta = Adt#adt.meta,
                          UpdatedMeta = Meta#meta{state = NextState},
			  Adt#adt{meta = UpdatedMeta}
            end,
            embryosys_storage_server:store(Type, Names, UpdatedAdt),
            {ok, UpdatedAdt};
        true ->
            {wrong_state, []}
    end.

%%

