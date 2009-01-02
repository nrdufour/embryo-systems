/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.core;

import java.util.Iterator;
import java.util.Map;
import java.util.TreeMap;

/**
 * @author nrdufour
 * 
 */
public class Adt {

	private AdtId id;
	private AdtState state = AdtState.NONE;
	private String name;
	private Map<String, String> extra = new TreeMap<String, String>();

	/**
	 * @return the id
	 */
	public AdtId getId() {
		return id;
	}

	/**
	 * @param id
	 *            the id to set
	 */
	public void setId(AdtId id) {
		this.id = id;
	}

	/**
	 * @return the state
	 */
	public AdtState getState() {
		return state;
	}

	/**
	 * @param state
	 *            the state to set
	 */
	public void setState(AdtState state) {
		this.state = state;
	}

	/**
	 * @return the name
	 */
	public String getName() {
		return name;
	}

	/**
	 * @param name
	 *            the name to set
	 */
	public void setName(String name) {
		this.name = name;
	}

	public void setField(final String key, final String value) {
		this.extra.put(key, value);
	}

	public void removeField(final String key) {
		this.extra.remove(key);
	}

	public String getField(final String key) {
		return this.extra.get(key);
	}

	public void clearFields() {
		this.extra.clear();
	}

	public Iterator<String> fieldKeysIterator() {
		return this.extra.keySet().iterator();
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see java.lang.Object#toString()
	 */
	@Override
	public String toString() {
		StringBuffer buffer = new StringBuffer();

		buffer.append("Adt(");
		buffer.append(this.id);
		buffer.append(", state: ");
		buffer.append(this.state);
		buffer.append(", name: ");
		buffer.append(this.name);
		buffer.append(", extra: ");
		buffer.append(this.extra);
		buffer.append(")");

		return buffer.toString();
	}
}
